use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;

use crate::config::Config;
use crate::tools::wallet::sign_serialized_transaction_base64;

pub const DEFAULT_AGENT_MERIDIAN_BASE: &str = "https://api.agentmeridian.xyz/api";
pub const DEFAULT_AGENT_ID: &str = "agent-local";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMeridianSettings {
    pub base_url: String,
    pub api_key: Option<String>,
    pub agent_id: String,
    pub lp_agent_relay_enabled: bool,
}

impl AgentMeridianSettings {
    pub fn from_config(config: &Config) -> Self {
        let base_url = config
            .api
            .agent_meridian_base
            .clone()
            .unwrap_or_else(|| DEFAULT_AGENT_MERIDIAN_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        let api_key = config
            .api
            .agent_meridian_key
            .clone()
            .or_else(|| std::env::var("PUBLIC_API_KEY").ok())
            .or_else(|| std::env::var("LPAGENT_API_KEY").ok())
            .filter(|key| !key.trim().is_empty());
        let agent_id = std::env::var("MERIDIAN_AGENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());

        Self {
            base_url,
            api_key,
            agent_id,
            lp_agent_relay_enabled: config.api.lp_agent_relay_enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayReplacementStatus {
    pub deploy_path: &'static str,
    pub close_claim_path: &'static str,
    pub swap_path: &'static str,
    pub lpagent_relay_enabled_default: bool,
    pub readonly_agent_meridian_endpoints: &'static [&'static str],
    pub notes: &'static str,
}

pub fn relay_replacement_status() -> RelayReplacementStatus {
    RelayReplacementStatus {
        deploy_path: "native_meteora_sdk",
        close_claim_path: "native_meteora_sdk",
        swap_path: "native_jupiter",
        lpagent_relay_enabled_default: false,
        readonly_agent_meridian_endpoints: &["/top-lp/:pool", "/study-top-lp/:pool"],
        notes: "Rust executes deploy/claim/close through the native Meteora SDK adapter and swaps through native Jupiter signing/submission; zap-in relay is documented as replaced. Agent Meridian remains supported for read-only LPAgent top-LPer/study endpoints and for signing stored zap-in/zap-out order payloads in tests/tools.",
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayTransactionGroups {
    #[serde(default)]
    pub close: Vec<String>,
    #[serde(default)]
    pub swap: Vec<String>,
    #[serde(default)]
    pub add_liquidity: Vec<String>,
}

impl RelayTransactionGroups {
    fn is_empty(&self) -> bool {
        self.close.is_empty() && self.swap.is_empty() && self.add_liquidity.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedRelayOrder {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_valid_block_height: Option<u64>,
    pub transactions: RelayTransactionGroups,
}

pub fn sign_zap_out_order(
    order: &serde_json::Value,
    keypair: &Keypair,
) -> Result<SignedRelayOrder> {
    let request_id = required_string(order, &["requestId"])?;
    let last_valid_block_height = order
        .pointer("/order/lastValidBlockHeight")
        .and_then(|value| value.as_u64());
    let unsigned = RelayTransactionGroups {
        close: string_array(order, &["order", "transactions", "close"]),
        swap: string_array(order, &["order", "transactions", "swap"]),
        add_liquidity: vec![],
    };
    sign_relay_order(
        request_id,
        last_valid_block_height,
        unsigned,
        keypair,
        "zap-out",
    )
}

pub fn sign_zap_in_order(order: &serde_json::Value, keypair: &Keypair) -> Result<SignedRelayOrder> {
    let request_id = required_string(order, &["requestId"])?;
    let last_valid_block_height = order
        .pointer("/order/lastValidBlockHeight")
        .and_then(|value| value.as_u64());
    let unsigned = RelayTransactionGroups {
        close: vec![],
        swap: string_array(order, &["order", "transactions", "swap"]),
        add_liquidity: string_array(order, &["order", "transactions", "addLiquidity"]),
    };
    sign_relay_order(
        request_id,
        last_valid_block_height,
        unsigned,
        keypair,
        "zap-in",
    )
}

fn sign_relay_order(
    request_id: String,
    last_valid_block_height: Option<u64>,
    unsigned: RelayTransactionGroups,
    keypair: &Keypair,
    label: &str,
) -> Result<SignedRelayOrder> {
    if unsigned.is_empty() {
        anyhow::bail!("Agent Meridian {} order returned no transactions", label);
    }

    Ok(SignedRelayOrder {
        request_id,
        last_valid_block_height,
        transactions: RelayTransactionGroups {
            close: sign_transaction_list(&unsigned.close, keypair)?,
            swap: sign_transaction_list(&unsigned.swap, keypair)?,
            add_liquidity: sign_transaction_list(&unsigned.add_liquidity, keypair)?,
        },
    })
}

fn sign_transaction_list(unsigned: &[String], keypair: &Keypair) -> Result<Vec<String>> {
    unsigned
        .iter()
        .map(|tx| sign_serialized_transaction_base64(tx, keypair))
        .collect()
}

fn required_string(value: &serde_json::Value, path: &[&str]) -> Result<String> {
    let current = value_at_path(value, path)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("missing required field {}", path.join(".")))?;
    Ok(current.to_string())
}

fn string_array(value: &serde_json::Value, path: &[&str]) -> Vec<String> {
    value_at_path(value, path)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|item| !item.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn value_at_path<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::message::{Message, VersionedMessage};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, Signature};
    use solana_sdk::signer::Signer;
    use solana_sdk::transaction::{Transaction, VersionedTransaction};

    use crate::config::{types::ApiConfig, Config};

    fn legacy_tx_base64(keypair: &Keypair) -> String {
        let message = Message::new(
            &[Instruction::new_with_bytes(
                Pubkey::new_unique(),
                &[],
                vec![AccountMeta::new_readonly(keypair.pubkey(), true)],
            )],
            Some(&keypair.pubkey()),
        );
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = Hash::new_unique();
        STANDARD.encode(bincode::serialize(&tx).expect("serialize legacy tx"))
    }

    fn versioned_tx_base64(keypair: &Keypair) -> String {
        let message = VersionedMessage::Legacy(Message::new(
            &[Instruction::new_with_bytes(
                Pubkey::new_unique(),
                &[],
                vec![AccountMeta::new_readonly(keypair.pubkey(), true)],
            )],
            Some(&keypair.pubkey()),
        ));
        let tx = VersionedTransaction {
            signatures: vec![
                Signature::default();
                message.header().num_required_signatures as usize
            ],
            message,
        };
        STANDARD.encode(bincode::serialize(&tx).expect("serialize versioned tx"))
    }

    #[test]
    fn settings_from_config_trim_base_url_and_keep_api_key() {
        let config = Config {
            api: ApiConfig {
                agent_meridian_base: Some("https://agent.example.test/api///".to_string()),
                agent_meridian_key: Some("public-test-key".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let settings = AgentMeridianSettings::from_config(&config);

        assert_eq!(settings.base_url, "https://agent.example.test/api");
        assert_eq!(settings.api_key.as_deref(), Some("public-test-key"));
        assert_eq!(settings.agent_id, "agent-local");
    }

    #[test]
    fn sign_zap_out_order_signs_close_and_swap_transaction_groups() {
        let keypair = Keypair::new();
        let order = serde_json::json!({
            "requestId": "req-123",
            "order": {
                "lastValidBlockHeight": 123456,
                "transactions": {
                    "close": [legacy_tx_base64(&keypair)],
                    "swap": [versioned_tx_base64(&keypair)]
                }
            }
        });

        let signed = sign_zap_out_order(&order, &keypair).expect("relay order should sign");

        assert_eq!(signed.request_id, "req-123");
        assert_eq!(signed.transactions.close.len(), 1);
        assert_eq!(signed.transactions.swap.len(), 1);
        assert_eq!(signed.last_valid_block_height, Some(123456));
    }

    #[test]
    fn sign_zap_out_order_rejects_empty_transaction_groups() {
        let keypair = Keypair::new();
        let order = serde_json::json!({
            "requestId": "req-empty",
            "order": { "transactions": { "close": [], "swap": [] } }
        });

        let err = sign_zap_out_order(&order, &keypair).expect_err("empty order should fail");

        assert!(err.to_string().contains("returned no transactions"));
    }

    #[test]
    fn settings_from_config_captures_lpagent_relay_flag() {
        let config = Config {
            api: ApiConfig {
                lp_agent_relay_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let settings = AgentMeridianSettings::from_config(&config);

        assert!(settings.lp_agent_relay_enabled);
    }

    #[test]
    fn relay_replacement_status_documents_native_and_readonly_paths() {
        let status = relay_replacement_status();

        assert_eq!(status.deploy_path, "native_meteora_sdk");
        assert_eq!(status.close_claim_path, "native_meteora_sdk");
        assert_eq!(status.swap_path, "native_jupiter");
        assert!(!status.lpagent_relay_enabled_default);
        assert!(status
            .readonly_agent_meridian_endpoints
            .contains(&"/top-lp/:pool"));
        assert!(status
            .readonly_agent_meridian_endpoints
            .contains(&"/study-top-lp/:pool"));
        assert!(status
            .notes
            .contains("zap-in relay is documented as replaced"));
    }
}
