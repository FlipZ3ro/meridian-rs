use anyhow::Result;
use reqwest::Client;
use serde_json::json;

/// Send a text message to the configured Telegram chat.
pub async fn send_message(bot_token: &str, chat_id: &str, text: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let client = Client::new();

    let resp = client
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
            "disable_web_page_preview": true,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Telegram sendMessage failed: {}", body);
    }

    Ok(())
}

/// Send a message, falling back to plain text if Markdown fails.
pub async fn send_message_safe(bot_token: &str, chat_id: &str, text: &str) -> Result<()> {
    match send_message(bot_token, chat_id, text).await {
        Ok(()) => Ok(()),
        Err(_) => {
            // Retry without markdown
            let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
            let client = Client::new();
            client
                .post(&url)
                .json(&json!({
                    "chat_id": chat_id,
                    "text": text,
                    "disable_web_page_preview": true,
                }))
                .send()
                .await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_message_signature() {
        // Verify the function signatures compile
        fn _check_sig(_: impl std::future::Future<Output = Result<()>>) {}
        _check_sig(send_message("token", "chat", "msg"));
        _check_sig(send_message_safe("token", "chat", "msg"));
    }
}
