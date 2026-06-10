use crate::utils::logger::module::warn;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: ChatMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub message: Option<String>,
    pub code: Option<u64>,
}

pub struct LlmClient {
    client: Client,
    pub api_key: String,
    pub base_url: String,
}

impl LlmClient {
    pub fn new(api_key: &str, base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("Failed to create HTTP client"),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
        }
    }

    pub async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        tool_choice: Option<serde_json::Value>,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatResponse> {
        let request = ChatRequest {
            model: model.to_string(),
            messages: messages.to_vec(),
            tools: tools.map(|t| t.to_vec()),
            tool_choice,
            temperature,
            max_tokens,
        };

        // Retry up to 3 times on transient errors
        for attempt in 0..3 {
            let resp = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
                .context("LLM request failed")?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                if status.as_u16() >= 500 || status.as_u16() == 429 {
                    let wait = (attempt + 1) * 3;
                    warn(
                        "llm",
                        &format!(
                            "LLM API {} retrying in {}s: {}",
                            status,
                            wait,
                            &text[..text.len().min(100)]
                        ),
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(anyhow::anyhow!("LLM API error {}: {}", status, text));
            }

            let chat_resp: ChatResponse = resp.json().await?;
            if chat_resp.choices.is_empty() {
                if let Some(ref err) = chat_resp.error {
                    let code = err.code.unwrap_or(0);
                    if code == 502 || code == 503 || code == 529 {
                        let wait = (attempt + 1) * 5;
                        warn(
                            "llm",
                            &format!("LLM provider error {}, retrying in {}s", code, wait),
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                        continue;
                    }
                }
                return Err(anyhow::anyhow!(
                    "LLM returned no choices: {:?}",
                    chat_resp.error
                ));
            }
            return Ok(chat_resp);
        }
        Err(anyhow::anyhow!("LLM request failed after 3 retries"))
    }

    /// Simple chat without tools (for screening)
    pub async fn chat(&self, model: &str, prompt: &str) -> Result<String> {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Some(prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        let resp = self
            .chat_with_tools(model, &messages, None, None, 0.7, 4096)
            .await?;
        Ok(resp
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_else(|| "No response".to_string()))
    }
}
