use crate::agent::prompt::{build_system_prompt, AgentRole};
use crate::agent::roles::get_tools_for_role;
use crate::config::{meridian_data_path, Config};
use crate::lessons::LessonStore;
use crate::llm::{ChatMessage, LlmClient, ToolDefinition};
use crate::signal_weights::SignalWeightsStore;
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::PositionState;
use crate::tools::executor::ToolExecutor;
use crate::utils::logger::module::{info, warn};
use anyhow::Result;
use serde_json::json;

/// Build tool definitions for OpenAI tool calling format
fn build_tool_definitions(tool_names: &[String]) -> Vec<ToolDefinition> {
    let all_tools = crate::tools::definitions::get_all_tool_definitions();
    all_tools
        .into_iter()
        .filter(|t| tool_names.contains(&t.function.name))
        .collect()
}

pub struct AgentRunContext<'a> {
    pub config: &'a Config,
    pub llm: &'a LlmClient,
    pub positions: &'a mut PositionState,
    pub pool_memory: &'a mut PoolMemoryStore,
    pub wallet_address: &'a str,
}

pub struct AgentLoop {
    pub max_steps: u32,
}

impl AgentLoop {
    pub fn new() -> Self {
        Self { max_steps: 20 }
    }

    /// Run the ReAct agent loop
    pub async fn run(
        &self,
        goal: &str,
        role: AgentRole,
        ctx: AgentRunContext<'_>,
    ) -> Result<String> {
        let AgentRunContext {
            config,
            llm,
            positions,
            pool_memory,
            wallet_address,
        } = ctx;

        info(
            "agent",
            &format!(
                "Agent loop starting — role={}, goal={}",
                role.as_str(),
                &goal[..goal.len().min(80)]
            ),
        );

        // Fetch real wallet balance for agent prompt
        let portfolio_json = {
            let rpc = config
                .api
                .helius_rpc_url
                .as_deref()
                .unwrap_or("https://api.mainnet-beta.solana.com");
            let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
            match crate::tools::wallet::get_wallet_balances(rpc, wallet_address, helius_key).await {
                Ok(bal) => serde_json::json!({"sol": bal.sol, "usd": bal.total_usd}).to_string(),
                Err(_) => r#"{"sol": 0, "note": "balance fetch failed"}"#.to_string(),
            }
        };
        let positions_json =
            serde_json::to_string(&positions.get_active()).unwrap_or_else(|_| "[]".to_string());
        let state_summary = pool_memory.get_summary_for_prompt();
        let lessons = load_learning_context(&role, config);
        let recent_decisions =
            crate::tools::executor::get_recent_decisions_for_prompt(5).unwrap_or_default();

        let system_prompt = build_system_prompt(
            &role,
            config,
            &portfolio_json,
            &positions_json,
            &state_summary,
            &lessons,
            &recent_decisions,
        );

        // Get tool set for this role
        let tool_names = get_tools_for_role(&role, goal);
        let tool_defs = build_tool_definitions(&tool_names);

        let mut executor = ToolExecutor::new(wallet_address);
        let mut messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(system_prompt),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(goal.to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let model = match role {
            AgentRole::Manager => &config.llm.management_model,
            AgentRole::Screener => &config.llm.screening_model,
            AgentRole::General => &config.llm.general_model,
        };

        let mut last_response = String::new();

        for step in 0..config.llm.max_steps {
            info(
                "agent",
                &format!("Step {}/{}", step + 1, config.llm.max_steps),
            );

            let tool_choice = if step == 0 && is_action_intent(goal) {
                json!("required")
            } else {
                json!("auto")
            };

            let response = llm
                .chat_with_tools(
                    model,
                    &messages,
                    Some(&tool_defs),
                    Some(tool_choice),
                    config.llm.temperature,
                    config.llm.max_tokens,
                )
                .await?;

            let choice = match response.choices.first() {
                Some(c) => c,
                None => {
                    warn("agent", "LLM returned no choices");
                    break;
                }
            };

            let msg = &choice.message;

            // If no tool calls, the agent is done
            if msg.tool_calls.is_none()
                || msg
                    .tool_calls
                    .as_ref()
                    .map(|t| t.is_empty())
                    .unwrap_or(true)
            {
                last_response = msg
                    .content
                    .clone()
                    .unwrap_or_else(|| "Agent completed with no text output.".to_string());
                messages.push(msg.clone());
                break;
            }

            // Process tool calls
            let tool_calls = msg.tool_calls.as_ref().unwrap();
            messages.push(msg.clone());

            for tc in tool_calls {
                info(
                    "agent",
                    &format!(
                        "Tool call: {}({})",
                        tc.function.name,
                        &tc.function.arguments[..tc.function.arguments.len().min(100)]
                    ),
                );

                let (result, is_error) = executor.execute(tc, config, positions, pool_memory).await;

                info(
                    "agent",
                    &format!(
                        "Tool result [{}]: {}",
                        if is_error { "ERR" } else { "OK" },
                        &result[..result.len().min(200)]
                    ),
                );

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                });
            }

            // Check if tool was required and didn't fire on step 0
            if step == 0 && is_action_intent(goal) && tool_calls.is_empty() {
                warn(
                    "agent",
                    "No tool call on action intent — agent may hallucinate",
                );
            }
        }

        Ok(last_response)
    }
}

fn load_learning_context(role: &AgentRole, config: &Config) -> String {
    let role_key = role.as_str().to_ascii_lowercase();
    let lessons_path = meridian_data_path("lessons.json");
    let mut sections = Vec::new();

    let lessons = lessons_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("lessons path is not UTF-8"))
        .and_then(LessonStore::load)
        .map(|store| store.get_rich_for_prompt(Some(&role_key), 12))
        .unwrap_or_default();
    if !lessons.trim().is_empty() {
        sections.push(lessons);
    }

    if matches!(role, AgentRole::Screener) && config.darwin.enabled {
        let weights_path = meridian_data_path("signal-weights.json");
        if let Ok(weights) = SignalWeightsStore::load(&weights_path) {
            sections.push(weights.summary());
        }
    }

    sections.join("\n\n")
}

fn is_action_intent(goal: &str) -> bool {
    let patterns = [
        "deploy",
        "open",
        "add liquidity",
        "close",
        "exit",
        "withdraw",
        "claim",
        "swap",
        "block",
        "unblock",
    ];
    let lower = goal.to_lowercase();
    patterns.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::LessonStore;
    use crate::signal_weights::SignalWeightsStore;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "meridian-{label}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn load_learning_context_injects_signal_weights_only_for_darwin_screener() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let dir = unique_test_dir("learning-context");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let _env = EnvGuard::set("MERIDIAN_DATA_DIR", &dir);

        let mut lessons = LessonStore::default();
        lessons.add_with_meta(
            "watch low organic entries",
            "screener",
            vec!["screening".to_string()],
            0.8,
            "manual",
        );
        lessons
            .save(&dir.join("lessons.json").to_string_lossy())
            .expect("lessons should save");
        let mut weights = SignalWeightsStore::default();
        weights.weights.insert("organic_score".to_string(), 1.25);
        weights.last_recalc = Some("2026-06-09T00:00:00Z".to_string());
        weights.recalc_count = 1;
        weights
            .save(&dir.join("signal-weights.json"))
            .expect("weights should save");

        let screener_context = load_learning_context(&AgentRole::Screener, &Config::default());
        let manager_context = load_learning_context(&AgentRole::Manager, &Config::default());
        let disabled = Config {
            darwin: crate::config::types::DarwinConfig {
                enabled: false,
                ..Default::default()
            },
            ..Config::default()
        };
        let disabled_context = load_learning_context(&AgentRole::Screener, &disabled);

        assert!(screener_context.contains("watch low organic entries"));
        assert!(screener_context.contains("Signal Weights"));
        assert!(screener_context.contains("organic_score"));
        assert!(!manager_context.contains("Signal Weights"));
        assert!(!disabled_context.contains("Signal Weights"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
