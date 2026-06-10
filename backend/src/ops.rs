use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StartupCheckStatus {
    Ok,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupCheck {
    pub name: String,
    pub status: StartupCheckStatus,
    pub message: String,
}

impl StartupCheck {
    pub fn ok(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: StartupCheckStatus::Ok,
            message: message.into(),
        }
    }

    pub fn warn(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: StartupCheckStatus::Warn,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupReport {
    pub checks: Vec<StartupCheck>,
}

impl StartupReport {
    pub fn has_warnings(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == StartupCheckStatus::Warn)
    }

    pub fn has_check(&self, name: &str, status: StartupCheckStatus) -> bool {
        self.checks
            .iter()
            .any(|check| check.name == name && check.status == status)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &StartupCheck> {
        self.checks
            .iter()
            .filter(|check| check.status == StartupCheckStatus::Warn)
    }
}

#[derive(Debug, Clone, Default)]
pub struct StartupEnv {
    vars: HashMap<String, String>,
}

impl StartupEnv {
    pub fn from_current() -> Self {
        Self {
            vars: std::env::vars().collect(),
        }
    }

    pub fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        Self {
            vars: pairs
                .into_iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    pub fn get_non_empty(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn first_non_empty(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().find_map(|key| self.get_non_empty(key))
    }
}

pub fn startup_report(config: &Config, state_path: &str, env: &StartupEnv) -> StartupReport {
    let mut checks = Vec::new();

    checks.push(if env.get_non_empty("MERIDIAN_CONFIG_PATH").is_some() {
        StartupCheck::ok("config_path", "MERIDIAN_CONFIG_PATH is set")
    } else {
        StartupCheck::warn(
            "config_path",
            "MERIDIAN_CONFIG_PATH is not set; runtime will rely on ~/.meridian/user-config.json or repo-local user-config.json",
        )
    });

    checks.push(
        if Path::new(state_path)
            .parent()
            .is_some_and(|parent| parent.exists())
        {
            StartupCheck::ok(
                "state_path",
                format!("state parent exists for {state_path}"),
            )
        } else {
            StartupCheck::warn(
                "state_path",
                format!("state parent does not exist yet for {state_path}"),
            )
        },
    );

    checks.push(if env
        .first_non_empty(&["WALLET_PRIVATE_KEY", "MERIDIAN_WALLET_PRIVATE_KEY"])
        .is_some()
    {
        StartupCheck::ok("wallet_private_key", "wallet private key env is present")
    } else {
        StartupCheck::warn(
            "wallet_private_key",
            "missing WALLET_PRIVATE_KEY or MERIDIAN_WALLET_PRIVATE_KEY; live deploy/claim/close/swap cannot sign",
        )
    });

    checks.push(if env.get_non_empty("MERIDIAN_WALLET").is_some() {
        StartupCheck::ok("wallet_address", "MERIDIAN_WALLET is set")
    } else {
        StartupCheck::warn(
            "wallet_address",
            "missing MERIDIAN_WALLET; read-only wallet commands need --wallet or this env",
        )
    });

    checks.push(
        if config
            .llm
            .api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
            || env
                .first_non_empty(&["LLM_API_KEY", "OPENROUTER_API_KEY"])
                .is_some()
        {
            StartupCheck::ok("llm_api_key", "LLM API key is configured")
        } else {
            StartupCheck::warn(
                "llm_api_key",
                "missing LLM_API_KEY or OPENROUTER_API_KEY; LLM cycles will fail",
            )
        },
    );

    checks.push(
        if config
            .api
            .helius_rpc_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
            || env
                .first_non_empty(&["RPC_URL", "HELIUS_RPC_URL"])
                .is_some()
        {
            StartupCheck::ok("rpc_url", "Solana RPC URL is configured")
        } else {
            StartupCheck::warn(
            "rpc_url",
            "missing RPC_URL or HELIUS_RPC_URL; Solana reads/writes will use weak defaults or fail",
        )
        },
    );

    StartupReport { checks }
}

pub fn check_port_available(name: &str, addr: &str) -> StartupCheck {
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            StartupCheck::ok(name, format!("{addr} is available"))
        }
        Err(error) => StartupCheck::warn(
            name,
            format!("{addr} is already in use or unavailable: {error}"),
        ),
    }
}

#[derive(Debug)]
pub struct ProcessGuard {
    path: PathBuf,
    active: bool,
}

impl ProcessGuard {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match create_lock_file(&path) {
            Ok(()) => Ok(Self { path, active: true }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if stale_lock_can_be_removed(&path)? {
                    std::fs::remove_file(&path).ok();
                    create_lock_file(&path)?;
                    Ok(Self { path, active: true })
                } else {
                    Err(anyhow!(
                        "Meridian already running or stale lock is active at {}",
                        path.display()
                    ))
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(&self.path);
            self.active = false;
        }
    }
}

fn create_lock_file(path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(())
}

fn stale_lock_can_be_removed(path: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let Some(pid) = content.trim().parse::<u32>().ok() else {
        return Ok(true);
    };
    Ok(!process_is_running(pid))
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn process_is_running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::net::TcpListener;

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("meridian-rs-ops-{}-{}", label, nanos))
    }

    #[test]
    fn env_template_is_trackable_and_contains_phase7_runtime_keys() {
        let template = include_str!("../.env.example");
        for required in [
            "DRY_RUN=true",
            "WALLET_PRIVATE_KEY=",
            "MERIDIAN_WALLET_PRIVATE_KEY=",
            "MERIDIAN_WALLET=",
            "RPC_URL=",
            "HELIUS_RPC_URL=",
            "HELIUS_API_KEY=",
            "LLM_BASE_URL=",
            "OPENROUTER_API_KEY=",
            "LLM_API_KEY=",
            "LLM_MODEL=",
            "MANAGEMENT_MODEL=",
            "SCREENING_MODEL=",
            "GENERAL_MODEL=",
            "MERIDIAN_DATA_DIR=",
            "MERIDIAN_STATE_PATH=",
            "MERIDIAN_WEB_ADDR=",
            "HEALTH_PORT=",
            "MERIDIAN_LOCK_PATH=",
            "JUPITER_API_KEY=",
            "AGENT_MERIDIAN_API_URL=",
            "PUBLIC_API_KEY=",
            "LPAGENT_API_KEY=",
        ] {
            assert!(
                template.contains(required),
                "missing env template key: {required}"
            );
        }

        let gitignore = include_str!("../.gitignore");
        assert!(gitignore.contains(".env.*"));
        assert!(gitignore.contains("!.env.example"));
    }

    #[test]
    fn production_docs_cover_encryption_deployment_and_native_replacements() {
        let docs = std::fs::read_to_string("docs/production-operations.md")
            .expect("production operations guide should exist");
        for required in [
            "launchd",
            "systemd",
            "PM2",
            "1Password",
            "sops",
            "MERIDIAN_LOCK_PATH",
            "Duplicate process",
            "Port conflict",
            "Claude Code slash-command replacement",
            "HiveMind/shared lessons replacement",
        ] {
            assert!(
                docs.contains(required),
                "missing production docs topic: {required}"
            );
        }
    }

    #[test]
    fn startup_report_flags_missing_required_runtime_inputs() {
        let config = Config {
            dry_run: true,
            ..Config::default()
        };
        let env = StartupEnv::from_pairs([]);
        let report = startup_report(&config, "state.json", &env);

        assert!(report.has_warnings());
        assert!(report.has_check("wallet_private_key", StartupCheckStatus::Warn));
        assert!(report.has_check("wallet_address", StartupCheckStatus::Warn));
        assert!(report.has_check("llm_api_key", StartupCheckStatus::Warn));
        assert!(report.has_check("rpc_url", StartupCheckStatus::Warn));
        assert!(report.has_check("config_path", StartupCheckStatus::Warn));
    }

    #[test]
    fn startup_report_passes_when_required_env_and_config_are_present() {
        let mut config = Config {
            dry_run: true,
            ..Config::default()
        };
        config.llm.api_key = Some("llm-key".to_string());
        config.api.helius_rpc_url = Some("https://rpc.example.test".to_string());
        let env = StartupEnv::from_pairs([
            ("WALLET_PRIVATE_KEY", "wallet-secret"),
            ("MERIDIAN_WALLET", "wallet-address"),
            ("MERIDIAN_CONFIG_PATH", "user-config.json"),
        ]);
        let report = startup_report(&config, "state.json", &env);

        assert!(report.has_check("wallet_private_key", StartupCheckStatus::Ok));
        assert!(report.has_check("wallet_address", StartupCheckStatus::Ok));
        assert!(report.has_check("llm_api_key", StartupCheckStatus::Ok));
        assert!(report.has_check("rpc_url", StartupCheckStatus::Ok));
    }

    #[test]
    fn process_guard_prevents_duplicate_lock_and_cleans_up_on_drop() {
        let dir = unique_test_dir("process-guard");
        std::fs::create_dir_all(&dir).expect("test dir");
        let lock_path = dir.join("meridian.lock");

        let guard = ProcessGuard::acquire(&lock_path).expect("first guard should acquire lock");
        assert!(lock_path.exists());
        let duplicate = ProcessGuard::acquire(&lock_path).expect_err("second guard should fail");
        assert!(duplicate.to_string().contains("already running"));

        drop(guard);
        assert!(!lock_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn port_check_reports_conflict_when_listener_already_bound() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("local addr").to_string();

        let check = check_port_available("web_port", &addr);
        assert_eq!(check.name, "web_port");
        assert_eq!(check.status, StartupCheckStatus::Warn);
        assert!(check.message.contains("already in use"));
    }
}
