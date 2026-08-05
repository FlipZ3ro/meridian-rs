use anyhow::Result;
use reqwest::Client;
use serde_json::json;

/// Tag identifying which instance sent a message, from
/// `MERIDIAN_INSTANCE_LABEL`. Several bots may share one token and one chat —
/// only `getUpdates` is exclusive, `sendMessage` is not — and without a tag
/// their notifications are indistinguishable once they land in the same chat.
/// Unset (the default) leaves messages exactly as they were.
fn instance_prefix() -> String {
    format_prefix(std::env::var("MERIDIAN_INSTANCE_LABEL").ok().as_deref())
}

fn format_prefix(label: Option<&str>) -> String {
    label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(|label| format!("[{}] ", label))
        .unwrap_or_default()
}

/// Send a text message to the configured Telegram chat.
pub async fn send_message(bot_token: &str, chat_id: &str, text: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let client = Client::new();

    let resp = client
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": format!("{}{}", instance_prefix(), text),
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
                    "text": format!("{}{}", instance_prefix(), text),
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

    /// Single-instance setups must look exactly as they did — the label only
    /// exists to separate bots that share one chat.
    #[test]
    fn a_message_is_only_tagged_when_an_instance_label_is_set() {
        assert_eq!(format_prefix(None), "");
        assert_eq!(format_prefix(Some("")), "");
        assert_eq!(format_prefix(Some("   ")), "", "a blank label is not one");
        assert_eq!(format_prefix(Some(" dual ")), "[dual] ");
    }
}
