use crate::agent::prompt::{build_system_prompt, AgentRole};
use crate::agent::roles::get_tools_for_role;
use crate::config::Config;
use crate::llm::{ChatMessage, LlmClient, ToolDefinition};
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
        config: &Config,
        llm: &LlmClient,
        positions: &mut PositionState,
        pool_memory: &mut PoolMemoryStore,
        wallet_address: &str,
    ) -> Result<String> {
        info(
            "agent",
            &format!(
                "Agent loop starting — role={}, goal={}",
                role.as_str(),
                &goal[..goal.len().min(80)]
            ),
        );

        let portfolio_json = r#"{"sol": 0, "note": "wallet stub"}"#;
        let positions_json =
            serde_json::to_string(&positions.get_active()).unwrap_or_else(|_| "[]".to_string());
        let state_summary = pool_memory.get_summary_for_prompt();
        let lessons = "";
        let perf_summary = "";

        let system_prompt = build_system_prompt(
            &role,
            config,
            portfolio_json,
            &positions_json,
            &state_summary,
            lessons,
            perf_summary,
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
