#![allow(dead_code)]

use anyhow::Result;
use chrono::Timelike;
use tokio::signal;
use tokio::sync::watch;

mod agent;
mod cli;
mod config;
mod cycle;
#[cfg(test)]
mod docs_quality;
mod lessons;
mod llm;
mod models;
mod ops;
mod signal_weights;
mod state;
mod strategy_library;
mod tools;
mod utils;
mod web;

use config::llm_config::LlmCredentials;
use config::{load_config, load_env_files, meridian_data_path};
use cycle::{
    queue_pnl_exit_close_instructions, run_management_cycle, run_pnl_poll, run_screening_cycle,
};
use llm::LlmClient;
use state::pool_memory::PoolMemoryStore;
use state::positions::PositionState;
use utils::logger::module::{info, warn};

#[derive(Debug, PartialEq, Eq)]
enum ReplCommandOutcome {
    Continue(Option<String>),
    Exit,
}

fn health_response() -> String {
    let body = r#"{"status":"ok","version":"0.2.0"}"#;
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

async fn run_repl_command(
    line: &str,
    config: &config::Config,
    state_path: &str,
    wallet_address: &str,
) -> Result<ReplCommandOutcome> {
    match line.trim() {
        "quit" | "exit" | "q" => Ok(ReplCommandOutcome::Exit),
        "help" | "h" => Ok(ReplCommandOutcome::Continue(Some(
            [
                "Commands:",
                "  status    — Show position state summary",
                "  screen    — Run screening cycle now",
                "  manage    — Run management cycle now",
                "  quit/exit — Graceful shutdown",
            ]
            .join("\n"),
        ))),
        "status" | "s" => {
            let positions = PositionState::load(state_path).unwrap_or_default();
            Ok(ReplCommandOutcome::Continue(Some(
                positions.get_state_summary(),
            )))
        }
        "screen" => {
            info("main", "Manual screening triggered");
            let output = cli::run_cli_command(
                cli::CliCommand::Screen {
                    wallet: Some(wallet_address.to_string()),
                    wallet_sol: None,
                },
                config,
                state_path,
            )
            .await?;
            Ok(ReplCommandOutcome::Continue(Some(output.render()?)))
        }
        "manage" | "m" => {
            info("main", "Manual management triggered");
            let output = cli::run_cli_command(
                cli::CliCommand::Manage {
                    wallet: Some(wallet_address.to_string()),
                },
                config,
                state_path,
            )
            .await?;
            Ok(ReplCommandOutcome::Continue(Some(output.render()?)))
        }
        "" => Ok(ReplCommandOutcome::Continue(None)),
        other => Ok(ReplCommandOutcome::Continue(Some(format!(
            "Unknown command: {other}. Type 'help' for commands."
        )))),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(command) = cli::parse_cli_args(&args)? {
        match &command {
            cli::CliCommand::Help => {
                println!("{}", cli::help_text());
                return Ok(());
            }
            cli::CliCommand::Setup { output_dir, force } => {
                let output_dir = output_dir.as_deref().unwrap_or(".");
                let summary = cli::run_setup_command(output_dir, *force)?;
                println!("{}", serde_json::to_string_pretty(&summary)?);
                return Ok(());
            }
            _ => {}
        }
        load_env_files();
        let config = load_config(None)?;
        let state_path = std::env::var("MERIDIAN_STATE_PATH").unwrap_or_else(|_| {
            meridian_data_path("meridian-state.json")
                .to_string_lossy()
                .into_owned()
        });
        let output = cli::run_cli_command(command, &config, &state_path).await?;
        println!("{}", output.render()?);
        return Ok(());
    }

    load_env_files();

    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .init();

    info(
        "main",
        "Meridian RS -- DLMM Liquidity Provider Agent v0.2.0",
    );
    info("main", "================================================");

    let config = load_config(None)?;
    info("main", "Config loaded");

    let creds = LlmCredentials::from_env_or_config(
        Some(&config.llm.base_url),
        config.llm.api_key.as_deref(),
    );
    info(
        "main",
        &format!("LLM client ready -- {}", &config.llm.base_url),
    );

    let state_path = std::env::var("MERIDIAN_STATE_PATH").unwrap_or_else(|_| {
        meridian_data_path("meridian-state.json")
            .to_string_lossy()
            .into_owned()
    });
    let pool_memory_path = meridian_data_path("pool-memory.json")
        .to_string_lossy()
        .into_owned();
    let health_port: u16 = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let lock_path = std::env::var("MERIDIAN_LOCK_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| meridian_data_path("meridian.lock"));
    let _process_guard = ops::ProcessGuard::acquire(&lock_path)?;
    info(
        "ops",
        &format!("Process lock acquired -- {}", lock_path.display()),
    );

    let startup_env = ops::StartupEnv::from_current();
    let startup_report = ops::startup_report(&config, &state_path, &startup_env);
    for check in &startup_report.checks {
        match check.status {
            ops::StartupCheckStatus::Ok => info("startup", &check.message),
            ops::StartupCheckStatus::Warn => warn("startup", &check.message),
        }
    }
    let web_addr =
        std::env::var("MERIDIAN_WEB_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    for check in [
        ops::check_port_available("web_port", &web_addr),
        ops::check_port_available("health_port", &format!("0.0.0.0:{health_port}")),
    ] {
        match check.status {
            ops::StartupCheckStatus::Ok => info("startup", &check.message),
            ops::StartupCheckStatus::Warn => warn("startup", &check.message),
        }
    }

    let positions = PositionState::load(&state_path)?;
    PoolMemoryStore::load(&pool_memory_path)?;
    info(
        "main",
        &format!(
            "State loaded -- {} active positions",
            positions.count_active()
        ),
    );

    // Read wallet address from env or config
    let wallet_address = std::env::var("MERIDIAN_WALLET").unwrap_or_else(|_| "".to_string());

    // ── Graceful shutdown channel ──────────────────────────────
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn Ctrl+C handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info("shutdown", "Ctrl+C received, shutting down gracefully...");
        let _ = shutdown_tx_clone.send(true);
    });

    // ── Web UI (Meridian OS) ───────────────────────────────────
    let mut shutdown_web = shutdown_rx.clone();
    tokio::spawn(async move {
        tokio::select! {
            result = web::start_web_server() => {
                if let Err(e) = result {
                    warn("web", &format!("Web UI stopped: {}", e));
                }
            }
            _ = shutdown_web.changed() => {
                info("web", "Shutdown signal received, stopping Web UI");
            }
        }
    });

    info("main", "Starting cycle scheduler...");

    // ── PnL Poller (every 30s, lightweight, no LLM) ───────────
    let pnl_interval =
        tokio::time::Duration::from_secs(config.schedule.pnl_poll_interval_secs as u64);
    let config_pnl = config.clone();
    let state_path_pnl = state_path.clone();
    let wallet_pnl = wallet_address.clone();
    let mut shutdown_pnl = shutdown_rx.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(pnl_interval);
        interval.tick().await; // skip first tick
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown_pnl.changed() => {
                    info("pnl_poll", "Shutdown signal received, stopping PnL poller");
                    break;
                }
            }

            let mut positions = match PositionState::load(&state_path_pnl) {
                Ok(p) => p,
                Err(e) => {
                    warn("pnl_poll", &format!("Failed to load positions: {}", e));
                    continue;
                }
            };

            match run_pnl_poll(&config_pnl, &mut positions, &wallet_pnl).await {
                Ok(exits) => {
                    if !exits.is_empty() {
                        for (addr, reason) in &exits {
                            info("pnl_poll", &format!("Exit needed: {} — {}", addr, reason));
                        }

                        let queued = queue_pnl_exit_close_instructions(&mut positions, &exits);
                        info(
                            "pnl_poll",
                            &format!(
                                "Queued {} close instruction(s) for the guarded management flow",
                                queued
                            ),
                        );

                        if let Err(e) = positions.save(&state_path_pnl) {
                            warn(
                                "pnl_poll",
                                &format!("Failed to save queued close instructions: {}", e),
                            );
                        }
                    }
                }
                Err(e) => {
                    warn("pnl_poll", &format!("PnL poll error: {}", e));
                }
            }
        }
    });

    // ── Management Cycle (every N minutes) ────────────────────
    let mgmt_interval =
        tokio::time::Duration::from_secs(config.schedule.management_interval_min as u64 * 60);
    let config_mgmt = config.clone();
    let llm_mgmt = LlmClient::new(&creds.api_key, &creds.base_url);
    let mgmt_state_path = state_path.clone();
    let mgmt_pool_memory_path = pool_memory_path.clone();
    let wallet_mgmt = wallet_address.clone();
    let mut shutdown_mgmt = shutdown_rx.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(mgmt_interval);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown_mgmt.changed() => {
                    info("mgmt", "Shutdown signal received, stopping management cycle");
                    break;
                }
            }

            let mut positions = match PositionState::load(&mgmt_state_path) {
                Ok(p) => p,
                Err(e) => {
                    warn("mgmt", &format!("Failed to load positions: {}", e));
                    continue;
                }
            };
            let mut pool_memory = match PoolMemoryStore::load(&mgmt_pool_memory_path) {
                Ok(pm) => pm,
                Err(e) => {
                    warn("mgmt", &format!("Failed to load pool memory: {}", e));
                    continue;
                }
            };

            match run_management_cycle(
                &config_mgmt,
                &llm_mgmt,
                &mut positions,
                &mut pool_memory,
                &wallet_mgmt,
            )
            .await
            {
                Ok(result) => {
                    info(
                        "mgmt",
                        &format!(
                            "Management cycle complete: {}",
                            &result[..result.len().min(200)]
                        ),
                    );
                    if let Err(e) = positions.save(&mgmt_state_path) {
                        warn("mgmt", &format!("Failed to save state: {}", e));
                    }
                }
                Err(e) => {
                    warn("mgmt", &format!("Management cycle error: {}", e));
                }
            }
        }
    });

    // ── Screening Cycle (every N minutes) ─────────────────────
    let screen_interval =
        tokio::time::Duration::from_secs(config.schedule.screening_interval_min as u64 * 60);
    let config_screen = config.clone();
    let llm_screen = LlmClient::new(&creds.api_key, &creds.base_url);
    let screen_state_path = state_path.clone();
    let screen_pool_memory_path = pool_memory_path.clone();
    let wallet_screen = wallet_address.clone();
    let mut shutdown_screen = shutdown_rx.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(screen_interval);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = shutdown_screen.changed() => {
                    info("screen", "Shutdown signal received, stopping screening cycle");
                    break;
                }
            }

            let mut positions = match PositionState::load(&screen_state_path) {
                Ok(p) => p,
                Err(e) => {
                    warn("screen", &format!("Failed to load positions: {}", e));
                    continue;
                }
            };
            let mut pool_memory = match PoolMemoryStore::load(&screen_pool_memory_path) {
                Ok(pm) => pm,
                Err(e) => {
                    warn("screen", &format!("Failed to load pool memory: {}", e));
                    continue;
                }
            };

            // Fetch real wallet SOL balance
            let wallet_sol = {
                let rpc = config_screen
                    .api
                    .helius_rpc_url
                    .as_deref()
                    .unwrap_or("https://api.mainnet-beta.solana.com");
                let helius_key = config_screen.api.helius_api_key.as_deref().unwrap_or("");
                match crate::tools::wallet::get_wallet_balances(rpc, &wallet_screen, helius_key)
                    .await
                {
                    Ok(balances) => balances.sol,
                    Err(e) => {
                        warn("screen", &format!("Failed to fetch wallet balance: {}", e));
                        0.0
                    }
                }
            };

            match run_screening_cycle(
                &config_screen,
                &llm_screen,
                &mut positions,
                &mut pool_memory,
                wallet_sol,
                &wallet_screen,
            )
            .await
            {
                Ok(result) => {
                    info(
                        "screen",
                        &format!(
                            "Screening cycle complete: {}",
                            &result[..result.len().min(200)]
                        ),
                    );
                }
                Err(e) => {
                    warn("screen", &format!("Screening cycle error: {}", e));
                }
            }

            // Save state after screening (in case deploy happened)
            if let Err(e) = positions.save(&screen_state_path) {
                warn("screen", &format!("Failed to save state: {}", e));
            }
        }
    });

    // ── Briefing Cycle (daily at 01:00 UTC) ─────────────────────
    let config_brief = config.clone();
    let state_brief = state_path.clone();
    let mut shutdown_brief = shutdown_rx.clone();

    tokio::spawn(async move {
        // Run briefing once at startup if it's between 01:00-02:00 UTC
        let now = chrono::Utc::now();
        let hour = now.hour();
        let minute = now.minute();
        let mut ran_today = hour == 1 && minute < 30;

        let interval = tokio::time::Duration::from_secs(3600); // check every hour
        let mut tick = tokio::time::interval(interval);
        tick.tick().await;

        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = shutdown_brief.changed() => {
                    info("briefing", "Shutdown signal received");
                    break;
                }
            }

            let now = chrono::Utc::now();
            if now.hour() == 1 && !ran_today {
                ran_today = true;

                // Build briefing
                let positions = match PositionState::load(&state_brief) {
                    Ok(p) => p,
                    Err(e) => {
                        warn("briefing", &format!("Failed to load positions: {}", e));
                        continue;
                    }
                };

                let state_summary = positions.get_state_summary();
                let active = positions.get_active();
                let active_count = active.len();

                let briefing = format!(
                    "📊 *Meridian Daily Briefing*\n{}\n\n*Open Positions:* {}\n\n{}",
                    now.format("%Y-%m-%d %H:%M UTC"),
                    active_count,
                    state_summary,
                );

                // Send via Telegram if configured
                if let (Some(bot_token), Some(chat_id)) = (
                    config_brief.api.telegram_bot_token.as_deref(),
                    config_brief.api.telegram_chat_id.as_deref(),
                ) {
                    if let Err(e) =
                        crate::tools::telegram::send_message_safe(bot_token, chat_id, &briefing)
                            .await
                    {
                        warn("briefing", &format!("Telegram send failed: {}", e));
                    } else {
                        info("briefing", "Daily briefing sent to Telegram");
                    }
                }

                // Reset ran_today at midnight UTC
                if now.hour() == 23 {
                    ran_today = false;
                }
            }
        }
    });

    // ── Health check endpoint (TCP listener) ───────────────────
    let mut shutdown_health = shutdown_rx.clone();

    tokio::spawn(async move {
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], health_port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                warn("health", &format!("Health bind failed: {}", e));
                return;
            }
        };
        info("health", &format!("Health check on :{}", health_port));

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        let resp = health_response();
                        use tokio::io::AsyncWriteExt;
                        let _ = stream.write_all(resp.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                }
                _ = shutdown_health.changed() => {
                    info("health", "Health check shutting down");
                    break;
                }
            }
        }
    });

    // ── REPL (interactive mode) ────────────────────────────────
    let is_tty = atty::is(atty::Stream::Stdin);
    if is_tty {
        info(
            "main",
            "Interactive mode — type 'help' for commands, 'quit' to exit",
        );
        let mut shutdown_repl = shutdown_rx.clone();

        loop {
            tokio::select! {
                _ = shutdown_repl.changed() => {
                    info("main", "Shutdown signal received");
                    break;
                }
                line = tokio::task::spawn_blocking(|| {
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).ok();
                    input.trim().to_string()
                }) => {
                    let line = line.unwrap_or_default();
                    match run_repl_command(&line, &config, &state_path, &wallet_address).await {
                        Ok(ReplCommandOutcome::Exit) => {
                            info("main", "Exiting...");
                            let _ = shutdown_tx.send(true);
                            break;
                        }
                        Ok(ReplCommandOutcome::Continue(Some(output))) => {
                            println!("{}", output);
                        }
                        Ok(ReplCommandOutcome::Continue(None)) => {}
                        Err(e) => {
                            warn("main", &format!("Manual command failed: {}", e));
                            println!("Manual command failed: {}", e);
                        }
                    }
                }
            }
        }
    } else {
        // Non-interactive: wait for shutdown signal
        info(
            "main",
            "Non-interactive mode, waiting for shutdown signal...",
        );
        let _ = shutdown_rx.clone().changed().await;
    }

    // ── Cleanup ────────────────────────────────────────────────
    info("main", "Saving final state...");
    if let Err(e) = positions.save(&state_path) {
        warn("main", &format!("Failed to save final state: {}", e));
    }
    info("main", "Meridian RS shutdown complete. Goodbye! 🧙");

    Ok(())
}

#[cfg(test)]
mod repl_tests {
    use super::*;
    use crate::config::Config;

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("meridian-rs-repl-{}-{}", label, nanos))
    }

    #[tokio::test]
    async fn repl_screen_command_runs_real_one_shot_screen_cycle() {
        let dir = unique_test_dir("screen");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let outcome = run_repl_command(
            "screen",
            &config,
            state_path.to_str().expect("state path should be utf8"),
            "",
        )
        .await
        .expect("manual screen should run without network when wallet is empty");

        let ReplCommandOutcome::Continue(Some(output)) = outcome else {
            panic!("screen should print JSON output and continue");
        };
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid JSON output");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "screen");
        assert!(parsed["data"]["result"]
            .as_str()
            .expect("screen result should be present")
            .contains("Not enough SOL"));
        assert!(!output.contains("Would run screening cycle"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn repl_manage_command_runs_real_one_shot_management_cycle() {
        let dir = unique_test_dir("manage");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let outcome = run_repl_command(
            "manage",
            &config,
            state_path.to_str().expect("state path should be utf8"),
            "Wallet111",
        )
        .await
        .expect("manual manage should run against empty local state");

        let ReplCommandOutcome::Continue(Some(output)) = outcome else {
            panic!("manage should print JSON output and continue");
        };
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid JSON output");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "manage");
        assert_eq!(parsed["data"]["result"], "No active positions.");
        assert!(!output.contains("Would run management cycle"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn health_response_has_content_length_for_clean_curl_smoke() {
        let response = health_response();

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Length: 33"));
        assert!(response.ends_with(r#"{"status":"ok","version":"0.2.0"}"#));
    }
}
