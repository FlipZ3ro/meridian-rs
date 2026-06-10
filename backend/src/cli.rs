use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::config::{load_config, resolve_config_path, save_config, Config};
use crate::cycle::{run_management_cycle, run_screening_cycle};
use crate::lessons::{EvolutionConfig, LessonStore, PerformanceInput};
use crate::llm::LlmClient;
use crate::signal_weights::{recalculate_weights, SignalWeightsStore};
use crate::state::pool_memory::PoolMemoryStore;
use crate::state::positions::PositionState;
use crate::tools::blacklist::{BlacklistEntry, BlacklistStore, BlockedDevEntry};
use crate::tools::discord_signals::{DiscordSignalRecord, DiscordSignalStatus, DiscordSignalStore};
use crate::tools::dlmm::{
    claim_fees, close_position, deploy_position, get_my_positions, get_position_pnl,
};
use crate::tools::screening::Screener;
use crate::tools::study::study_top_lpers;
use crate::tools::wallet::{get_wallet_balances, swap_token};

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigAction {
    Get { key: Option<String> },
    Set { key: String, value: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LessonAction {
    List {
        role: Option<String>,
    },
    Add {
        content: String,
        role: Option<String>,
        tags: Vec<String>,
    },
    Pin {
        id: String,
    },
    Unpin {
        id: String,
    },
    Clear,
    Prompt {
        role: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PerformanceAction {
    Summary,
    List {
        limit: Option<usize>,
    },
    Record {
        position_id: String,
        pool: String,
        symbol: String,
        pnl_sol: f64,
        fees_earned: f64,
        range_efficiency: f64,
        close_reason: String,
        signal_snapshot: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PoolMemoryAction {
    Summary,
    List,
    Show {
        pool: String,
    },
    AddNote {
        pool: String,
        base_mint: String,
        symbol: Option<String>,
        note: String,
    },
    Cooldown {
        base_mint: String,
        reason: String,
        minutes: u32,
    },
    ClearCooldown {
        base_mint: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlacklistAction {
    List,
    Add {
        mint: String,
        symbol: Option<String>,
        reason: Option<String>,
    },
    Remove {
        mint: String,
    },
    DevList,
    DevAdd {
        wallet: String,
        label: Option<String>,
        reason: Option<String>,
    },
    DevRemove {
        wallet: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscordSignalsAction {
    List,
    Clear,
    Queue {
        pool: String,
        base_mint: String,
        symbol: Option<String>,
        author: Option<String>,
        channel: Option<String>,
        snippet: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrategyAction {
    List,
    Show { id: String },
    SetActive { id: String },
    Remove { id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliCommand {
    Help,
    Setup {
        output_dir: Option<String>,
        force: bool,
    },
    Config {
        action: ConfigAction,
        file: Option<String>,
    },
    Lessons {
        action: LessonAction,
    },
    Performance {
        action: PerformanceAction,
    },
    Evolve,
    PoolMemory {
        action: PoolMemoryAction,
    },
    Blacklist {
        action: BlacklistAction,
    },
    DiscordSignals {
        action: DiscordSignalsAction,
    },
    Strategies {
        action: StrategyAction,
    },
    Status,
    Balance {
        wallet: Option<String>,
    },
    Positions {
        wallet: Option<String>,
    },
    Pnl {
        pool: String,
        position: String,
        wallet: Option<String>,
    },
    Candidates {
        limit: Option<usize>,
    },
    Study {
        pool: String,
        limit: Option<usize>,
    },
    Screen {
        wallet: Option<String>,
        wallet_sol: Option<f64>,
    },
    Manage {
        wallet: Option<String>,
    },
    Deploy {
        pool: String,
        amount_sol: f64,
        bins_below: Option<i64>,
        bins_above: Option<i64>,
        strategy: Option<String>,
        dry_run: bool,
    },
    Claim {
        position: String,
    },
    Close {
        position: String,
        reason: Option<String>,
        skip_swap: bool,
    },
    Swap {
        mint: String,
        amount: f64,
    },
}

pub enum CliOutput {
    Json(Value),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupSummary {
    pub success: bool,
    pub env_path: PathBuf,
    pub config_path: PathBuf,
    pub env_written: bool,
    pub config_written: bool,
    pub message: String,
}

const ENV_TEMPLATE: &str = include_str!("../.env.example");
const USER_CONFIG_TEMPLATE: &str = include_str!("../user-config.example.json");

impl CliOutput {
    pub fn render(&self) -> Result<String> {
        match self {
            Self::Json(value) => Ok(serde_json::to_string_pretty(value)?),
            Self::Text(text) => Ok(text.clone()),
        }
    }
}

pub fn help_text() -> String {
    [
        "Meridian RS command center",
        "──────────────────────────",
        "Usage:",
        "  meridian <command> [options]",
        "",
        "Core runtime",
        "  meridian setup [--dir <path>] [--force]",
        "      Generate .env and user-config.json templates.",
        "  meridian status",
        "      Print active-position summary as JSON.",
        "  meridian screen [--wallet <addr>] [--wallet-sol <sol>]",
        "      Run one screening cycle now.",
        "  meridian manage [--wallet <addr>]",
        "      Run one management cycle now.",
        "",
        "Trading",
        "  meridian balance [--wallet <addr>]",
        "  meridian positions [--wallet <addr>]",
        "  meridian pnl --pool <pool> --position <position> [--wallet <addr>]",
        "  meridian candidates [--limit <n>]",
        "  meridian study --pool <addr> [--limit <n>]",
        "  meridian deploy --pool <pool> --amount <sol> [--bins-below <n>] [--bins-above <n>] [--strategy spot|curve|bid_ask] [--dry-run]",
        "  meridian claim --position <position>",
        "  meridian close --position <position> [--reason <text>] [--skip-swap]",
        "  meridian swap --from <mint> --amount <tokens>",
        "",
        "State & learning",
        "  meridian config get [key] [--file <path>]",
        "  meridian config set <key> <value> [--file <path>]",
        "  meridian lessons list|add|pin|unpin|clear|prompt ...",
        "  meridian performance summary|list|record ...",
        "  meridian evolve",
        "  meridian pool-memory summary|list|show|add-note|cooldown|clear-cooldown ...",
        "  meridian blacklist list|add|remove|dev-list|dev-add|dev-remove ...",
        "  meridian discord-signals [clear|queue --pool <pool> --base-mint <mint> [--symbol <sym>]]",
        "  meridian strategies [list|show|set-active|remove] [id]",
        "",
        "No subcommand starts the long-running agent runtime.",
        "Set MERIDIAN_LOG_STYLE=pretty for readable operator logs, or plain for legacy log lines.",
    ]
    .join("\n")
}

pub fn parse_cli_args(args: &[String]) -> Result<Option<CliCommand>> {
    let Some(command) = args.get(1).map(String::as_str) else {
        return Ok(None);
    };

    let tail = &args[2..];
    match command {
        "help" | "--help" | "-h" => Ok(Some(CliCommand::Help)),
        "setup" => Ok(Some(CliCommand::Setup {
            output_dir: optional_flag(tail, &["--dir", "--output-dir"]),
            force: has_flag(tail, "--force"),
        })),
        "config" => parse_config_args(tail).map(Some),
        "lessons" | "lesson" => parse_lessons_args(tail).map(Some),
        "performance" | "perf" => parse_performance_args(tail).map(Some),
        "evolve" => Ok(Some(CliCommand::Evolve)),
        "pool-memory" | "pool_memory" => parse_pool_memory_args(tail).map(Some),
        "blacklist" | "blocklist" => parse_blacklist_args(tail).map(Some),
        "discord-signals" | "discord_signals" => parse_discord_signals_args(tail).map(Some),
        "strategy" | "strategies" => parse_strategy_args(tail).map(Some),
        "status" => Ok(Some(CliCommand::Status)),
        "balance" => Ok(Some(CliCommand::Balance {
            wallet: optional_flag(tail, &["--wallet", "-w"]),
        })),
        "positions" => Ok(Some(CliCommand::Positions {
            wallet: optional_flag(tail, &["--wallet", "-w"]),
        })),
        "pnl" => Ok(Some(CliCommand::Pnl {
            pool: required_flag(tail, &["--pool", "--pool-address"])?
                .ok_or_else(|| anyhow!("pnl requires --pool <pool>"))?,
            position: required_flag(tail, &["--position", "--position-address"])?
                .ok_or_else(|| anyhow!("pnl requires --position <position>"))?,
            wallet: optional_flag(tail, &["--wallet", "-w"]),
        })),
        "candidates" => Ok(Some(CliCommand::Candidates {
            limit: optional_flag(tail, &["--limit", "-n"])
                .map(|value| parse_usize("--limit", &value))
                .transpose()?,
        })),
        "study" => Ok(Some(CliCommand::Study {
            pool: required_flag(tail, &["--pool", "--pool-address"])?
                .ok_or_else(|| anyhow!("study requires --pool <pool>"))?,
            limit: optional_flag(tail, &["--limit", "-n"])
                .map(|value| parse_usize("--limit", &value))
                .transpose()?,
        })),
        "screen" | "screening" => Ok(Some(CliCommand::Screen {
            wallet: optional_flag(tail, &["--wallet", "-w"]),
            wallet_sol: optional_flag(tail, &["--wallet-sol", "--sol-balance"])
                .map(|value| parse_f64("--wallet-sol", &value))
                .transpose()?,
        })),
        "manage" | "management" => Ok(Some(CliCommand::Manage {
            wallet: optional_flag(tail, &["--wallet", "-w"]),
        })),
        "deploy" => Ok(Some(CliCommand::Deploy {
            pool: required_flag(tail, &["--pool", "--pool-address"])?
                .ok_or_else(|| anyhow!("deploy requires --pool <pool>"))?,
            amount_sol: required_flag(
                tail,
                &["--amount", "--amount-sol", "--amount_y", "--amount-y"],
            )?
            .ok_or_else(|| anyhow!("deploy requires --amount <sol>"))?
            .parse::<f64>()
            .map_err(|e| anyhow!("invalid --amount: {}", e))?,
            bins_below: optional_flag(tail, &["--bins-below"])
                .map(|value| parse_i64("--bins-below", &value))
                .transpose()?,
            bins_above: optional_flag(tail, &["--bins-above"])
                .map(|value| parse_i64("--bins-above", &value))
                .transpose()?,
            strategy: optional_flag(tail, &["--strategy"]),
            dry_run: has_flag(tail, "--dry-run"),
        })),
        "claim" => Ok(Some(CliCommand::Claim {
            position: required_flag(tail, &["--position", "--position-address"])?
                .ok_or_else(|| anyhow!("claim requires --position <position>"))?,
        })),
        "close" => Ok(Some(CliCommand::Close {
            position: required_flag(tail, &["--position", "--position-address"])?
                .ok_or_else(|| anyhow!("close requires --position <position>"))?,
            reason: optional_flag(tail, &["--reason"]),
            skip_swap: has_flag(tail, "--skip-swap"),
        })),
        "swap" => Ok(Some(CliCommand::Swap {
            mint: required_flag(tail, &["--from", "--mint", "--input-mint", "--from-mint"])?
                .ok_or_else(|| anyhow!("swap requires --from <mint>"))?,
            amount: required_flag(tail, &["--amount"])?
                .ok_or_else(|| anyhow!("swap requires --amount <tokens>"))?
                .parse::<f64>()
                .map_err(|e| anyhow!("invalid --amount: {}", e))?,
        })),
        _ => Err(anyhow!("unknown command '{}'. Use 'help'.", command)),
    }
}

fn parse_config_args(args: &[String]) -> Result<CliCommand> {
    let file = required_flag(args, &["--file", "--config"])?;
    let mut positionals = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "--file" | "--config") {
            if args.get(index + 1).is_none() {
                return Err(anyhow!("{} requires a value", arg));
            }
            index += 2;
            continue;
        }
        if arg.starts_with("--file=") || arg.starts_with("--config=") {
            index += 1;
            continue;
        }
        positionals.push(arg.clone());
        index += 1;
    }

    let action = match positionals.first().map(String::as_str) {
        Some("get") => ConfigAction::Get {
            key: positionals.get(1).cloned(),
        },
        Some("set") => ConfigAction::Set {
            key: positionals
                .get(1)
                .cloned()
                .ok_or_else(|| anyhow!("config set requires <key> <value>"))?,
            value: positionals
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow!("config set requires <key> <value>"))?,
        },
        Some(other) => {
            return Err(anyhow!(
                "unknown config action '{}'. Use get or set.",
                other
            ))
        }
        None => return Err(anyhow!("config requires get or set")),
    };

    Ok(CliCommand::Config { action, file })
}

fn parse_lessons_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("list");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "list" => LessonAction::List {
            role: optional_flag(action_args, &["--role"]),
        },
        "add" => LessonAction::Add {
            content: positional_or_flag(
                action_args,
                &["--content", "--text"],
                "lessons add requires content",
            )?,
            role: optional_flag(action_args, &["--role"]),
            tags: optional_flag(action_args, &["--tags"])
                .map(|tags| split_csv(&tags))
                .unwrap_or_default(),
        },
        "pin" => LessonAction::Pin {
            id: positional_or_flag(action_args, &["--id"], "lessons pin requires id")?,
        },
        "unpin" => LessonAction::Unpin {
            id: positional_or_flag(action_args, &["--id"], "lessons unpin requires id")?,
        },
        "clear" => LessonAction::Clear,
        "prompt" => LessonAction::Prompt {
            role: optional_flag(action_args, &["--role"]),
        },
        other => return Err(anyhow!("unknown lessons action '{}'", other)),
    };
    Ok(CliCommand::Lessons { action })
}

fn parse_performance_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("summary");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "summary" => PerformanceAction::Summary,
        "list" => PerformanceAction::List {
            limit: optional_flag(action_args, &["--limit", "-n"])
                .map(|value| parse_usize("--limit", &value))
                .transpose()?,
        },
        "record" => PerformanceAction::Record {
            position_id: required_flag(action_args, &["--position", "--position-id"])?
                .ok_or_else(|| anyhow!("performance record requires --position <id>"))?,
            pool: required_flag(action_args, &["--pool"])?
                .ok_or_else(|| anyhow!("performance record requires --pool <pool>"))?,
            symbol: required_flag(action_args, &["--symbol"])?
                .ok_or_else(|| anyhow!("performance record requires --symbol <symbol>"))?,
            pnl_sol: required_flag(action_args, &["--pnl", "--pnl-sol"])?
                .ok_or_else(|| anyhow!("performance record requires --pnl <sol>"))
                .and_then(|value| parse_f64("--pnl", &value))?,
            fees_earned: optional_flag(action_args, &["--fees", "--fees-earned"])
                .map(|value| parse_f64("--fees", &value))
                .transpose()?
                .unwrap_or(0.0),
            range_efficiency: optional_flag(action_args, &["--range-efficiency", "--range"])
                .map(|value| parse_f64("--range-efficiency", &value))
                .transpose()?
                .unwrap_or(0.0),
            close_reason: optional_flag(action_args, &["--reason", "--close-reason"])
                .unwrap_or_else(|| "manual".to_string()),
            signal_snapshot: optional_flag(action_args, &["--signals", "--signal-snapshot"])
                .unwrap_or_else(|| "{}".to_string()),
        },
        other => return Err(anyhow!("unknown performance action '{}'", other)),
    };
    Ok(CliCommand::Performance { action })
}

fn parse_pool_memory_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("summary");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "summary" => PoolMemoryAction::Summary,
        "list" => PoolMemoryAction::List,
        "show" => PoolMemoryAction::Show {
            pool: positional_or_flag(action_args, &["--pool"], "pool-memory show requires pool")?,
        },
        "add-note" | "note" => PoolMemoryAction::AddNote {
            pool: required_flag(action_args, &["--pool", "--pool-address"])?
                .ok_or_else(|| anyhow!("pool-memory add-note requires --pool <pool>"))?,
            base_mint: required_flag(action_args, &["--base-mint", "--mint"])?
                .ok_or_else(|| anyhow!("pool-memory add-note requires --base-mint <mint>"))?,
            symbol: optional_flag(action_args, &["--symbol"]),
            note: required_flag(action_args, &["--note"])?
                .ok_or_else(|| anyhow!("pool-memory add-note requires --note <text>"))?,
        },
        "cooldown" => PoolMemoryAction::Cooldown {
            base_mint: required_flag(action_args, &["--base-mint", "--mint"])?
                .ok_or_else(|| anyhow!("pool-memory cooldown requires --base-mint <mint>"))?,
            reason: optional_flag(action_args, &["--reason"])
                .unwrap_or_else(|| "manual".to_string()),
            minutes: optional_flag(action_args, &["--minutes"])
                .map(|value| parse_u32("--minutes", &value))
                .transpose()?
                .unwrap_or(60),
        },
        "clear-cooldown" => PoolMemoryAction::ClearCooldown {
            base_mint: positional_or_flag(
                action_args,
                &["--base-mint", "--mint"],
                "pool-memory clear-cooldown requires base mint",
            )?,
        },
        other => return Err(anyhow!("unknown pool-memory action '{}'", other)),
    };
    Ok(CliCommand::PoolMemory { action })
}

fn parse_blacklist_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("list");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "list" => BlacklistAction::List,
        "add" => BlacklistAction::Add {
            mint: positional_or_flag(action_args, &["--mint"], "blacklist add requires mint")?,
            symbol: optional_flag(action_args, &["--symbol"]),
            reason: optional_flag(action_args, &["--reason"]),
        },
        "remove" | "rm" => BlacklistAction::Remove {
            mint: positional_or_flag(action_args, &["--mint"], "blacklist remove requires mint")?,
        },
        "dev-list" | "devs" => BlacklistAction::DevList,
        "dev-add" | "block-dev" => BlacklistAction::DevAdd {
            wallet: positional_or_flag(
                action_args,
                &["--wallet"],
                "blacklist dev-add requires wallet",
            )?,
            label: optional_flag(action_args, &["--label"]),
            reason: optional_flag(action_args, &["--reason"]),
        },
        "dev-remove" | "unblock-dev" => BlacklistAction::DevRemove {
            wallet: positional_or_flag(
                action_args,
                &["--wallet"],
                "blacklist dev-remove requires wallet",
            )?,
        },
        other => return Err(anyhow!("unknown blacklist action '{}'", other)),
    };
    Ok(CliCommand::Blacklist { action })
}

fn parse_discord_signals_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("list");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "list" => DiscordSignalsAction::List,
        "clear" => DiscordSignalsAction::Clear,
        "queue" | "add" => DiscordSignalsAction::Queue {
            pool: required_flag(action_args, &["--pool", "--pool-address"])?
                .ok_or_else(|| anyhow!("discord-signals queue requires --pool <pool>"))?,
            base_mint: required_flag(action_args, &["--base-mint", "--mint"])?
                .ok_or_else(|| anyhow!("discord-signals queue requires --base-mint <mint>"))?,
            symbol: optional_flag(action_args, &["--symbol"]),
            author: optional_flag(action_args, &["--author"]),
            channel: optional_flag(action_args, &["--channel"]),
            snippet: optional_flag(action_args, &["--snippet", "--message"]),
        },
        other => return Err(anyhow!("unknown discord-signals action '{}'", other)),
    };
    Ok(CliCommand::DiscordSignals { action })
}

fn parse_strategy_args(args: &[String]) -> Result<CliCommand> {
    let action_name = args.first().map(String::as_str).unwrap_or("list");
    let action_args = if args.is_empty() { args } else { &args[1..] };
    let action = match action_name {
        "list" => StrategyAction::List,
        "show" | "get" => StrategyAction::Show {
            id: positional_or_flag(action_args, &["--id"], "strategy show requires id")?,
        },
        "set-active" | "active" | "use" => StrategyAction::SetActive {
            id: positional_or_flag(action_args, &["--id"], "strategy set-active requires id")?,
        },
        "remove" | "rm" => StrategyAction::Remove {
            id: positional_or_flag(action_args, &["--id"], "strategy remove requires id")?,
        },
        other => return Err(anyhow!("unknown strategy action '{}'", other)),
    };
    Ok(CliCommand::Strategies { action })
}

pub fn run_setup_command(output_dir: impl AsRef<Path>, force: bool) -> Result<SetupSummary> {
    let output_dir = output_dir.as_ref();
    let env_path = output_dir.join(".env");
    let config_path = output_dir.join("user-config.json");

    let mut existing = Vec::new();
    if env_path.exists() {
        existing.push(env_path.display().to_string());
    }
    if config_path.exists() {
        existing.push(config_path.display().to_string());
    }
    if !force && !existing.is_empty() {
        return Err(anyhow!(
            "setup target already exists: {} (pass --force to overwrite)",
            existing.join(", ")
        ));
    }

    std::fs::create_dir_all(output_dir)?;
    std::fs::write(&env_path, ENV_TEMPLATE)?;
    std::fs::write(&config_path, USER_CONFIG_TEMPLATE)?;

    Ok(SetupSummary {
        success: true,
        env_path,
        config_path,
        env_written: true,
        config_written: true,
        message: "Generated .env and user-config.json templates".to_string(),
    })
}

pub fn command_name(command: &CliCommand) -> &'static str {
    match command {
        CliCommand::Help => "help",
        CliCommand::Setup { .. } => "setup",
        CliCommand::Config { .. } => "config",
        CliCommand::Lessons { .. } => "lessons",
        CliCommand::Performance { .. } => "performance",
        CliCommand::Evolve => "evolve",
        CliCommand::PoolMemory { .. } => "pool-memory",
        CliCommand::Blacklist { .. } => "blacklist",
        CliCommand::DiscordSignals { .. } => "discord-signals",
        CliCommand::Strategies { .. } => "strategies",
        CliCommand::Status => "status",
        CliCommand::Balance { .. } => "balance",
        CliCommand::Positions { .. } => "positions",
        CliCommand::Pnl { .. } => "pnl",
        CliCommand::Candidates { .. } => "candidates",
        CliCommand::Study { .. } => "study",
        CliCommand::Screen { .. } => "screen",
        CliCommand::Manage { .. } => "manage",
        CliCommand::Deploy { .. } => "deploy",
        CliCommand::Claim { .. } => "claim",
        CliCommand::Close { .. } => "close",
        CliCommand::Swap { .. } => "swap",
    }
}

pub fn json_command_output(command: &str, data: Value) -> CliOutput {
    CliOutput::Json(json!({
        "success": true,
        "command": command,
        "data": data,
    }))
}

fn json_command_result(command: &str, data: impl Serialize) -> Result<CliOutput> {
    Ok(json_command_output(command, serde_json::to_value(data)?))
}

pub fn run_config_command(
    action: ConfigAction,
    current_config: &Config,
    file: Option<&Path>,
) -> Result<CliOutput> {
    let config_path = resolve_config_path(file.and_then(Path::to_str));
    let config = if file.is_some() {
        load_config(config_path.to_str())?
    } else {
        current_config.clone()
    };

    match action {
        ConfigAction::Get { key } => {
            let value = config_value_at(&config, key.as_deref())?;
            Ok(json_command_output(
                "config",
                json!({
                    "action": "get",
                    "key": key,
                    "value": value,
                    "configPath": config_path,
                }),
            ))
        }
        ConfigAction::Set { key, value } => {
            let parsed_value = parse_config_value(&value);
            let mut serialized = serde_json::to_value(&config)?;
            set_config_value_at(&mut serialized, &key, parsed_value.clone())?;
            let updated: Config = serde_json::from_value(serialized)?;
            save_config(&updated, config_path.to_str())?;
            Ok(json_command_output(
                "config",
                json!({
                    "action": "set",
                    "key": key,
                    "value": parsed_value,
                    "configPath": config_path,
                }),
            ))
        }
    }
}

fn config_value_at(config: &Config, key: Option<&str>) -> Result<Value> {
    let value = serde_json::to_value(config)?;
    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
        return Ok(value);
    };

    let mut cursor = &value;
    for part in key.split('.') {
        cursor = cursor
            .get(part)
            .ok_or_else(|| anyhow!("unknown config key '{}'", key))?;
    }
    Ok(cursor.clone())
}

fn set_config_value_at(config: &mut Value, key: &str, new_value: Value) -> Result<()> {
    let parts: Vec<&str> = key.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return Err(anyhow!("config key cannot be empty"));
    }

    let mut cursor = config;
    for part in &parts[..parts.len() - 1] {
        cursor = cursor
            .get_mut(*part)
            .ok_or_else(|| anyhow!("unknown config key '{}'", key))?;
        if !cursor.is_object() {
            return Err(anyhow!("config key '{}' is not an object", part));
        }
    }

    let leaf = parts[parts.len() - 1];
    let Some(object) = cursor.as_object_mut() else {
        return Err(anyhow!("config parent for '{}' is not an object", key));
    };
    if !object.contains_key(leaf) {
        return Err(anyhow!("unknown config key '{}'", key));
    }
    object.insert(leaf.to_string(), new_value);
    Ok(())
}

fn parse_config_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn data_dir_for_state(state_path: &str) -> PathBuf {
    Path::new(state_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn lessons_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("lessons.json")
}

fn token_blacklist_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("token-blacklist.json")
}

fn dev_blocklist_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("dev-blocklist.json")
}

fn discord_signals_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("discord-signals.json")
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn save_lesson_store(store: &LessonStore, path: &Path) -> Result<()> {
    ensure_parent(path)?;
    store.save(
        path.to_str()
            .ok_or_else(|| anyhow!("lesson path is not UTF-8"))?,
    )
}

fn run_lessons_command(action: LessonAction, state_path: &str) -> Result<CliOutput> {
    let path = lessons_path_for_state(state_path);
    let mut store = LessonStore::load(
        path.to_str()
            .ok_or_else(|| anyhow!("lesson path is not UTF-8"))?,
    )?;
    match action {
        LessonAction::List { role } => {
            let lessons: Vec<_> = store
                .lessons
                .iter()
                .filter(|lesson| {
                    role.as_deref()
                        .is_none_or(|role| lesson.role.as_deref() == Some(role))
                })
                .collect();
            Ok(json_command_output(
                "lessons",
                json!({"action": "list", "count": lessons.len(), "lessons": lessons, "path": path.display().to_string()}),
            ))
        }
        LessonAction::Add {
            content,
            role,
            tags,
        } => {
            if let Some(role) = role.as_deref() {
                store.add_with_meta(&content, role, tags, 0.5, "manual");
            } else {
                store.add(&content);
            }
            let lesson = store.lessons.last().cloned();
            save_lesson_store(&store, &path)?;
            Ok(json_command_output(
                "lessons",
                json!({"action": "add", "count": store.lessons.len(), "lesson": lesson, "path": path.display().to_string()}),
            ))
        }
        LessonAction::Pin { id } => {
            let updated = store.pin(&id);
            save_lesson_store(&store, &path)?;
            Ok(json_command_output(
                "lessons",
                json!({"action": "pin", "id": id, "updated": updated, "count": store.lessons.len(), "path": path.display().to_string()}),
            ))
        }
        LessonAction::Unpin { id } => {
            let updated = store.unpin(&id);
            save_lesson_store(&store, &path)?;
            Ok(json_command_output(
                "lessons",
                json!({"action": "unpin", "id": id, "updated": updated, "count": store.lessons.len(), "path": path.display().to_string()}),
            ))
        }
        LessonAction::Clear => {
            store.clear();
            save_lesson_store(&store, &path)?;
            Ok(json_command_output(
                "lessons",
                json!({"action": "clear", "count": store.lessons.len(), "path": path.display().to_string()}),
            ))
        }
        LessonAction::Prompt { role } => Ok(json_command_output(
            "lessons",
            json!({"action": "prompt", "role": role, "prompt": store.get_for_prompt(role.as_deref()), "path": path.display().to_string()}),
        )),
    }
}

fn run_performance_command(action: PerformanceAction, state_path: &str) -> Result<CliOutput> {
    let path = lessons_path_for_state(state_path);
    let mut store = LessonStore::load(
        path.to_str()
            .ok_or_else(|| anyhow!("lesson path is not UTF-8"))?,
    )?;
    match action {
        PerformanceAction::Summary => Ok(json_command_output(
            "performance",
            json!({"action": "summary", "summary": store.get_performance_summary(), "performanceCount": store.performance.len(), "closeCount": store.close_count, "path": path.display().to_string()}),
        )),
        PerformanceAction::List { limit } => {
            let mut records = store.performance.clone();
            records.reverse();
            if let Some(limit) = limit {
                records.truncate(limit);
            }
            Ok(json_command_output(
                "performance",
                json!({"action": "list", "count": records.len(), "performance": records, "path": path.display().to_string()}),
            ))
        }
        PerformanceAction::Record {
            position_id,
            pool,
            symbol,
            pnl_sol,
            fees_earned,
            range_efficiency,
            close_reason,
            signal_snapshot,
        } => {
            store.record_performance(PerformanceInput {
                position_id: &position_id,
                pool: &pool,
                symbol: &symbol,
                pnl_sol,
                fees_earned,
                range_efficiency,
                close_reason: &close_reason,
                signal_snapshot: &signal_snapshot,
            });
            save_lesson_store(&store, &path)?;
            Ok(json_command_output(
                "performance",
                json!({"action": "record", "performanceCount": store.performance.len(), "closeCount": store.close_count, "last": store.performance.last(), "path": path.display().to_string()}),
            ))
        }
    }
}

fn run_evolve_command(config: &Config, state_path: &str) -> Result<CliOutput> {
    let path = lessons_path_for_state(state_path);
    let mut store = LessonStore::load(
        path.to_str()
            .ok_or_else(|| anyhow!("lesson path is not UTF-8"))?,
    )?;
    let evolution = EvolutionConfig {
        evolve_every_n_closes: (config.darwin.recalc_every as usize).max(1),
        boost_good: config.darwin.boost_factor,
        decay_bad: config.darwin.decay_factor,
        ..EvolutionConfig::default()
    };
    let result = store.evolve_thresholds(
        config.screening.min_organic,
        config.screening.min_fee_active_tvl_ratio,
        &evolution,
    );

    let signal_weights = if config.darwin.enabled {
        let weights_path = data_dir_for_state(state_path).join("signal-weights.json");
        let mut weights_store = SignalWeightsStore::load(&weights_path)?;
        let weights_result =
            recalculate_weights(&store.performance, &config.darwin, &mut weights_store);
        weights_store.save(&weights_path)?;
        Some(json!({
            "changes": weights_result.changes,
            "recalcCount": weights_store.recalc_count,
            "path": weights_path.display().to_string(),
            "summary": weights_store.summary(),
        }))
    } else {
        None
    };

    if result.is_some() {
        save_lesson_store(&store, &path)?;
    }
    let data = match result {
        Some((min_organic, min_fee_active_tvl_ratio, lesson)) => json!({
            "evolved": true,
            "minOrganic": min_organic,
            "minFeeActiveTvlRatio": min_fee_active_tvl_ratio,
            "lesson": lesson,
            "closeCount": store.close_count,
            "path": path.display().to_string(),
            "signalWeights": signal_weights,
        }),
        None => json!({
            "evolved": false,
            "closeCount": store.close_count,
            "performanceCount": store.performance.len(),
            "path": path.display().to_string(),
            "signalWeights": signal_weights,
        }),
    };
    Ok(json_command_output("evolve", data))
}

fn pool_memory_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("pool-memory.json")
}

fn run_pool_memory_command(action: PoolMemoryAction, state_path: &str) -> Result<CliOutput> {
    let path = pool_memory_path_for_state(state_path);
    let mut store = PoolMemoryStore::load(
        path.to_str()
            .ok_or_else(|| anyhow!("pool-memory path is not UTF-8"))?,
    )?;
    match action {
        PoolMemoryAction::Summary => Ok(json_command_output(
            "pool-memory",
            json!({"action": "summary", "summary": store.get_summary_for_prompt(), "poolCount": store.pools.len(), "path": path.display().to_string()}),
        )),
        PoolMemoryAction::List => Ok(json_command_output(
            "pool-memory",
            json!({"action": "list", "poolCount": store.pools.len(), "pools": store.pools, "path": path.display().to_string()}),
        )),
        PoolMemoryAction::Show { pool } => Ok(json_command_output(
            "pool-memory",
            json!({"action": "show", "pool": pool, "entry": store.get(&pool), "path": path.display().to_string()}),
        )),
        PoolMemoryAction::AddNote {
            pool,
            base_mint,
            symbol,
            note,
        } => {
            store.add_note(&pool, &base_mint, symbol.as_deref(), &note);
            ensure_parent(&path)?;
            store.save(
                path.to_str()
                    .ok_or_else(|| anyhow!("pool-memory path is not UTF-8"))?,
            )?;
            Ok(json_command_output(
                "pool-memory",
                json!({"action": "add-note", "pool": pool, "poolCount": store.pools.len(), "path": path.display().to_string()}),
            ))
        }
        PoolMemoryAction::Cooldown {
            base_mint,
            reason,
            minutes,
        } => {
            store.set_cooldown(&base_mint, &reason, minutes);
            ensure_parent(&path)?;
            store.save(
                path.to_str()
                    .ok_or_else(|| anyhow!("pool-memory path is not UTF-8"))?,
            )?;
            Ok(json_command_output(
                "pool-memory",
                json!({"action": "cooldown", "baseMint": base_mint, "minutes": minutes, "poolCount": store.pools.len(), "path": path.display().to_string()}),
            ))
        }
        PoolMemoryAction::ClearCooldown { base_mint } => {
            store.clear_cooldown(&base_mint);
            ensure_parent(&path)?;
            store.save(
                path.to_str()
                    .ok_or_else(|| anyhow!("pool-memory path is not UTF-8"))?,
            )?;
            Ok(json_command_output(
                "pool-memory",
                json!({"action": "clear-cooldown", "baseMint": base_mint, "poolCount": store.pools.len(), "path": path.display().to_string()}),
            ))
        }
    }
}

fn load_blacklist_store(state_path: &str) -> Result<BlacklistStore> {
    let token_path = token_blacklist_path_for_state(state_path);
    let mut store = if token_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&token_path)?)?
    } else {
        BlacklistStore {
            mints: Default::default(),
            blocked_devs: Default::default(),
        }
    };
    let dev_path = dev_blocklist_path_for_state(state_path);
    if dev_path.exists() {
        store.blocked_devs = serde_json::from_str(&std::fs::read_to_string(dev_path)?)?;
    }
    Ok(store)
}

fn save_token_blacklist_store(store: &BlacklistStore, state_path: &str) -> Result<()> {
    let path = token_blacklist_path_for_state(state_path);
    ensure_parent(&path)?;
    std::fs::write(path, serde_json::to_string_pretty(store)?)?;
    Ok(())
}

fn save_dev_blocklist_store(store: &BlacklistStore, state_path: &str) -> Result<()> {
    let path = dev_blocklist_path_for_state(state_path);
    ensure_parent(&path)?;
    std::fs::write(path, serde_json::to_string_pretty(&store.blocked_devs)?)?;
    Ok(())
}

fn run_blacklist_command(action: BlacklistAction, state_path: &str) -> Result<CliOutput> {
    let mut store = load_blacklist_store(state_path)?;
    match action {
        BlacklistAction::List => Ok(json_command_output(
            "blacklist",
            json!({"action": "list", "count": store.mints.len(), "blacklist": store.list_blacklist().blacklist, "path": token_blacklist_path_for_state(state_path).display().to_string()}),
        )),
        BlacklistAction::Add {
            mint,
            symbol,
            reason,
        } => {
            let entry = store
                .mints
                .entry(mint.clone())
                .or_insert_with(|| BlacklistEntry {
                    symbol: symbol.clone().unwrap_or_else(|| "UNKNOWN".to_string()),
                    reason: reason
                        .clone()
                        .unwrap_or_else(|| "no reason provided".to_string()),
                    added_at: chrono::Utc::now().to_rfc3339(),
                    added_by: "cli".to_string(),
                });
            let item = json!({"mint": mint, "symbol": entry.symbol, "reason": entry.reason});
            save_token_blacklist_store(&store, state_path)?;
            Ok(json_command_output(
                "blacklist",
                json!({"action": "add", "count": store.mints.len(), "item": item, "path": token_blacklist_path_for_state(state_path).display().to_string()}),
            ))
        }
        BlacklistAction::Remove { mint } => {
            let removed = store.mints.remove(&mint);
            save_token_blacklist_store(&store, state_path)?;
            Ok(json_command_output(
                "blacklist",
                json!({"action": "remove", "mint": mint, "removed": removed, "count": store.mints.len(), "path": token_blacklist_path_for_state(state_path).display().to_string()}),
            ))
        }
        BlacklistAction::DevList => Ok(json_command_output(
            "blacklist",
            json!({"action": "dev-list", "count": store.blocked_devs.len(), "blockedDevs": store.list_blocked_devs().blocked_devs, "path": dev_blocklist_path_for_state(state_path).display().to_string()}),
        )),
        BlacklistAction::DevAdd {
            wallet,
            label,
            reason,
        } => {
            let entry =
                store
                    .blocked_devs
                    .entry(wallet.clone())
                    .or_insert_with(|| BlockedDevEntry {
                        label: label.clone().unwrap_or_else(|| "unknown".to_string()),
                        reason: reason
                            .clone()
                            .unwrap_or_else(|| "no reason provided".to_string()),
                        added_at: chrono::Utc::now().to_rfc3339(),
                    });
            let item = json!({"wallet": wallet, "label": entry.label, "reason": entry.reason});
            save_dev_blocklist_store(&store, state_path)?;
            Ok(json_command_output(
                "blacklist",
                json!({"action": "dev-add", "count": store.blocked_devs.len(), "item": item, "path": dev_blocklist_path_for_state(state_path).display().to_string()}),
            ))
        }
        BlacklistAction::DevRemove { wallet } => {
            let removed = store.blocked_devs.remove(&wallet);
            save_dev_blocklist_store(&store, state_path)?;
            Ok(json_command_output(
                "blacklist",
                json!({"action": "dev-remove", "wallet": wallet, "removed": removed, "count": store.blocked_devs.len(), "path": dev_blocklist_path_for_state(state_path).display().to_string()}),
            ))
        }
    }
}

fn strategy_library_path_for_state(state_path: &str) -> PathBuf {
    data_dir_for_state(state_path).join("strategy-library.json")
}

fn run_discord_signals_command(
    action: DiscordSignalsAction,
    state_path: &str,
) -> Result<CliOutput> {
    let path = discord_signals_path_for_state(state_path);
    let mut store = DiscordSignalStore::load_at(&path)?;
    match action {
        DiscordSignalsAction::List => Ok(json_command_output(
            "discord-signals",
            json!(store.summary()),
        )),
        DiscordSignalsAction::Clear => {
            let result = store.clear_processed()?;
            Ok(json_command_output(
                "discord-signals",
                json!({"action": "clear", "cleared": result.cleared, "remaining": result.remaining, "path": result.path}),
            ))
        }
        DiscordSignalsAction::Queue {
            pool,
            base_mint,
            symbol,
            author,
            channel,
            snippet,
        } => {
            let now = chrono::Utc::now();
            let record = DiscordSignalRecord {
                id: format!(
                    "{}-{}",
                    pool.chars().take(8).collect::<String>(),
                    now.timestamp_millis()
                ),
                pool_address: pool,
                base_mint,
                base_symbol: symbol.unwrap_or_else(|| "?".to_string()),
                signal_source: "discord".to_string(),
                discord_guild: "manual".to_string(),
                discord_channel: channel.unwrap_or_else(|| "manual".to_string()),
                discord_author: author.unwrap_or_else(|| "manual".to_string()),
                discord_message_snippet: snippet.unwrap_or_default().chars().take(120).collect(),
                queued_at: now.to_rfc3339(),
                rug_score: None,
                total_fees_sol: None,
                token_age_minutes: None,
                status: DiscordSignalStatus::Pending,
                discovery_pool: None,
            };
            store.queue(record.clone())?;
            Ok(json_command_output(
                "discord-signals",
                json!({"action": "queue", "count": store.signals.len(), "pending": store.pending().len(), "signal": record, "path": path.display().to_string()}),
            ))
        }
    }
}

fn run_strategy_command(action: StrategyAction, state_path: &str) -> Result<CliOutput> {
    let path = strategy_library_path_for_state(state_path);
    let mut store = crate::strategy_library::StrategyLibraryStore::load(&path)?;
    match action {
        StrategyAction::List => {
            let list = store.list_strategies();
            Ok(json_command_output(
                "strategies",
                json!({
                    "action": "list",
                    "active": list.active,
                    "count": list.count,
                    "strategies": list.strategies,
                    "path": path.display().to_string(),
                }),
            ))
        }
        StrategyAction::Show { id } => {
            let strategy = store
                .get_strategy(&id)
                .ok_or_else(|| anyhow!("strategy '{}' not found", id))?;
            Ok(json_command_output(
                "strategies",
                json!({
                    "action": "show",
                    "id": id,
                    "strategy": strategy,
                    "active": store.active,
                    "path": path.display().to_string(),
                }),
            ))
        }
        StrategyAction::SetActive { id } => {
            let result = store.set_active_strategy(&id)?;
            store.save(&path)?;
            Ok(json_command_output(
                "strategies",
                json!({
                    "action": "set-active",
                    "result": result,
                    "path": path.display().to_string(),
                }),
            ))
        }
        StrategyAction::Remove { id } => {
            let result = store.remove_strategy(&id)?;
            store.save(&path)?;
            Ok(json_command_output(
                "strategies",
                json!({
                    "action": "remove",
                    "result": result,
                    "path": path.display().to_string(),
                }),
            ))
        }
    }
}

fn llm_client_from_config(config: &Config) -> LlmClient {
    LlmClient::new(
        config.llm.api_key.as_deref().unwrap_or(""),
        &config.llm.base_url,
    )
}

async fn resolve_wallet_sol(config: &Config, wallet: &str, wallet_sol: Option<f64>) -> Result<f64> {
    if let Some(wallet_sol) = wallet_sol {
        return Ok(wallet_sol);
    }
    if wallet.trim().is_empty() {
        return Ok(0.0);
    }
    let rpc = config
        .api
        .helius_rpc_url
        .as_deref()
        .unwrap_or("https://api.mainnet-beta.solana.com");
    let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
    Ok(get_wallet_balances(rpc, wallet, helius_key).await?.sol)
}

pub async fn run_cli_command(
    command: CliCommand,
    config: &Config,
    state_path: &str,
) -> Result<CliOutput> {
    match command {
        CliCommand::Help => Ok(CliOutput::Text(help_text())),
        CliCommand::Setup { output_dir, force } => {
            let output_dir = output_dir.unwrap_or_else(|| ".".to_string());
            Ok(CliOutput::Json(serde_json::to_value(run_setup_command(
                output_dir, force,
            )?)?))
        }
        CliCommand::Config { action, file } => {
            run_config_command(action, config, file.as_deref().map(Path::new))
        }
        CliCommand::Lessons { action } => run_lessons_command(action, state_path),
        CliCommand::Performance { action } => run_performance_command(action, state_path),
        CliCommand::Evolve => run_evolve_command(config, state_path),
        CliCommand::PoolMemory { action } => run_pool_memory_command(action, state_path),
        CliCommand::Blacklist { action } => run_blacklist_command(action, state_path),
        CliCommand::DiscordSignals { action } => run_discord_signals_command(action, state_path),
        CliCommand::Strategies { action } => run_strategy_command(action, state_path),
        CliCommand::Status => {
            let positions = PositionState::load(state_path).unwrap_or_default();
            Ok(CliOutput::Json(json!({
                "success": true,
                "active_positions": positions.count_active(),
                "summary": positions.get_state_summary(),
            })))
        }
        CliCommand::Balance { wallet } => {
            let wallet = wallet.or_else(env_wallet).ok_or_else(|| {
                anyhow!("wallet required: pass --wallet <addr> or set MERIDIAN_WALLET")
            })?;
            let rpc = config
                .api
                .helius_rpc_url
                .as_deref()
                .unwrap_or("https://api.mainnet-beta.solana.com");
            let helius_key = config.api.helius_api_key.as_deref().unwrap_or("");
            json_command_result(
                "balance",
                get_wallet_balances(rpc, &wallet, helius_key).await?,
            )
        }
        CliCommand::Positions { wallet } => {
            let wallet = wallet.or_else(env_wallet).ok_or_else(|| {
                anyhow!("wallet required: pass --wallet <addr> or set MERIDIAN_WALLET")
            })?;
            json_command_result("positions", get_my_positions(&wallet, config).await?)
        }
        CliCommand::Pnl {
            pool,
            position,
            wallet,
        } => {
            let wallet = wallet.or_else(env_wallet).ok_or_else(|| {
                anyhow!("wallet required: pass --wallet <addr> or set MERIDIAN_WALLET")
            })?;
            json_command_result("pnl", get_position_pnl(&pool, &position, &wallet).await?)
        }
        CliCommand::Candidates { limit } => {
            let screener = Screener::new();
            let result = screener
                .get_top_candidates_with_rejections(&config.screening, limit.unwrap_or(3))
                .await?;
            Ok(json_command_output(
                "candidates",
                json!({
                    "total_screened": result.total_screened,
                    "candidates": result.candidates,
                    "filtered_examples": result.filtered_examples,
                }),
            ))
        }
        CliCommand::Study { pool, limit } => json_command_result(
            "study",
            study_top_lpers(&pool, limit.unwrap_or(4), config).await?,
        ),
        CliCommand::Screen { wallet, wallet_sol } => {
            let mut positions = PositionState::load(state_path).unwrap_or_default();
            let pool_memory_path = pool_memory_path_for_state(state_path);
            let mut pool_memory =
                PoolMemoryStore::load(pool_memory_path.to_str().unwrap_or("pool-memory.json"))?;
            let wallet_address = wallet.or_else(env_wallet).unwrap_or_default();
            let resolved_wallet_sol =
                resolve_wallet_sol(config, &wallet_address, wallet_sol).await?;
            let llm = llm_client_from_config(config);
            let result = run_screening_cycle(
                config,
                &llm,
                &mut positions,
                &mut pool_memory,
                resolved_wallet_sol,
                &wallet_address,
            )
            .await?;
            positions.save(state_path)?;
            pool_memory.save(pool_memory_path.to_str().unwrap_or("pool-memory.json"))?;
            Ok(json_command_output(
                "screen",
                json!({
                    "result": result,
                    "activePositions": positions.count_active(),
                    "walletSol": resolved_wallet_sol,
                    "statePath": state_path,
                    "poolMemoryPath": pool_memory_path,
                }),
            ))
        }
        CliCommand::Manage { wallet } => {
            let mut positions = PositionState::load(state_path).unwrap_or_default();
            let pool_memory_path = pool_memory_path_for_state(state_path);
            let mut pool_memory =
                PoolMemoryStore::load(pool_memory_path.to_str().unwrap_or("pool-memory.json"))?;
            let wallet_address = wallet.or_else(env_wallet).unwrap_or_default();
            let llm = llm_client_from_config(config);
            let result = run_management_cycle(
                config,
                &llm,
                &mut positions,
                &mut pool_memory,
                &wallet_address,
            )
            .await?;
            positions.save(state_path)?;
            pool_memory.save(pool_memory_path.to_str().unwrap_or("pool-memory.json"))?;
            Ok(json_command_output(
                "manage",
                json!({
                    "result": result,
                    "activePositions": positions.count_active(),
                    "statePath": state_path,
                    "poolMemoryPath": pool_memory_path,
                }),
            ))
        }
        CliCommand::Deploy {
            pool,
            amount_sol,
            bins_below,
            bins_above,
            strategy,
            dry_run,
        } => {
            let mut effective_config = config.clone();
            if dry_run {
                effective_config.dry_run = true;
            }
            json_command_result(
                "deploy",
                deploy_position(
                    &pool,
                    amount_sol,
                    bins_below,
                    bins_above,
                    strategy.as_deref(),
                    &effective_config,
                )
                .await?,
            )
        }
        CliCommand::Claim { position } => {
            json_command_result("claim", claim_fees(&position, config).await?)
        }
        CliCommand::Close {
            position,
            reason,
            skip_swap: _,
        } => json_command_result(
            "close",
            close_position(&position, reason.as_deref(), config).await?,
        ),
        CliCommand::Swap { mint, amount } => {
            json_command_result("swap", swap_token(&mint, amount, 50, 100, config).await?)
        }
    }
}

fn env_wallet() -> Option<String> {
    std::env::var("MERIDIAN_WALLET")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn optional_flag(args: &[String], names: &[&str]) -> Option<String> {
    required_flag(args, names).ok().flatten()
}

fn positional_or_flag(args: &[String], names: &[&str], message: &str) -> Result<String> {
    if let Some(value) = optional_flag(args, names) {
        return Ok(value);
    }
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .cloned()
        .ok_or_else(|| anyhow!(message.to_string()))
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn required_flag(args: &[String], names: &[&str]) -> Result<Option<String>> {
    for (idx, arg) in args.iter().enumerate() {
        if names.iter().any(|name| arg == name) {
            let Some(value) = args.get(idx + 1) else {
                return Err(anyhow!("{} requires a value", arg));
            };
            if value.starts_with('-') {
                return Err(anyhow!("{} requires a value", arg));
            }
            return Ok(Some(value.clone()));
        }
        if let Some((name, value)) = arg.split_once('=') {
            if names.contains(&name) {
                if value.is_empty() {
                    return Err(anyhow!("{} requires a value", name));
                }
                return Ok(Some(value.to_string()));
            }
        }
    }
    Ok(None)
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn parse_f64(label: &str, value: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .map_err(|e| anyhow!("invalid {}: {}", label, e))
}

fn parse_i64(label: &str, value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|e| anyhow!("invalid {}: {}", label, e))
}

fn parse_u32(label: &str, value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|e| anyhow!("invalid {}: {}", label, e))
}

fn parse_usize(label: &str, value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .map_err(|e| anyhow!("invalid {}: {}", label, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn parse_no_args_means_agent_runtime() {
        let parsed = parse_cli_args(&args(&["meridian"])).expect("no args should parse");
        assert_eq!(parsed, None);
    }

    #[test]
    fn parse_deploy_subcommand_maps_original_js_flags() {
        let parsed = parse_cli_args(&args(&[
            "meridian",
            "deploy",
            "--pool",
            "Pool111",
            "--amount",
            "0.25",
            "--bins-below",
            "35",
            "--bins-above",
            "0",
            "--strategy",
            "bid_ask",
            "--dry-run",
        ]))
        .expect("deploy args should parse");

        assert_eq!(
            parsed,
            Some(CliCommand::Deploy {
                pool: "Pool111".to_string(),
                amount_sol: 0.25,
                bins_below: Some(35),
                bins_above: Some(0),
                strategy: Some("bid_ask".to_string()),
                dry_run: true,
            })
        );
    }

    #[test]
    fn parse_core_trading_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&["meridian", "claim", "--position", "Pos111"])).unwrap(),
            Some(CliCommand::Claim {
                position: "Pos111".to_string(),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "close",
                "--position",
                "Pos111",
                "--skip-swap",
                "--reason",
                "low yield",
            ]))
            .unwrap(),
            Some(CliCommand::Close {
                position: "Pos111".to_string(),
                reason: Some("low yield".to_string()),
                skip_swap: true,
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian", "swap", "--from", "Mint111", "--amount", "1.5"
            ]))
            .unwrap(),
            Some(CliCommand::Swap {
                mint: "Mint111".to_string(),
                amount: 1.5,
            })
        );
    }

    #[test]
    fn help_text_groups_commands_for_readability() {
        let help = help_text();

        for required in [
            "Meridian RS command center",
            "Core runtime",
            "Trading",
            "State & learning",
            "meridian deploy --pool <pool>",
            "No subcommand starts the long-running agent runtime.",
        ] {
            assert!(help.contains(required), "missing help section: {required}");
        }
    }

    #[test]
    fn parse_setup_command() {
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "setup",
                "--dir",
                "/tmp/meridian-setup",
                "--force",
            ]))
            .unwrap(),
            Some(CliCommand::Setup {
                output_dir: Some("/tmp/meridian-setup".to_string()),
                force: true,
            })
        );
    }

    #[test]
    fn setup_generates_env_and_user_config_templates() {
        let dir = unique_test_dir("setup-generates");
        let summary = run_setup_command(&dir, false).expect("setup should write templates");

        let env_path = dir.join(".env");
        let config_path = dir.join("user-config.json");
        assert_eq!(summary.env_path, env_path);
        assert_eq!(summary.config_path, config_path);
        assert!(summary.env_written);
        assert!(summary.config_written);

        let env = std::fs::read_to_string(&env_path).expect(".env should be readable");
        assert!(env.contains("DRY_RUN=true"));
        assert!(env.contains("WALLET_PRIVATE_KEY="));
        assert!(env.contains("LLM_BASE_URL=https://openrouter.ai/api/v1"));

        let config = std::fs::read_to_string(&config_path).expect("config should be readable");
        let parsed: serde_json::Value = serde_json::from_str(&config).expect("valid json config");
        assert_eq!(parsed["screening"]["timeframe"], "5m");
        assert_eq!(parsed["management"]["deployAmountSol"], 0.5);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn setup_refuses_to_overwrite_existing_files_without_force() {
        let dir = unique_test_dir("setup-no-overwrite");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let env_path = dir.join(".env");
        std::fs::write(&env_path, "KEEP_ME=1\n").expect("seed env should be written");

        let err = run_setup_command(&dir, false).expect_err("setup should not overwrite .env");
        assert!(err.to_string().contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(&env_path).expect("env should remain readable"),
            "KEEP_ME=1\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("meridian-rs-{}-{}", label, nanos))
    }

    #[test]
    fn parse_readonly_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&["meridian", "balance", "--wallet", "Wallet111"])).unwrap(),
            Some(CliCommand::Balance {
                wallet: Some("Wallet111".to_string()),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["meridian", "positions"])).unwrap(),
            Some(CliCommand::Positions { wallet: None })
        );
        assert_eq!(
            parse_cli_args(&args(&["meridian", "candidates", "--limit", "7"])).unwrap(),
            Some(CliCommand::Candidates { limit: Some(7) })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian", "study", "--pool", "Pool111", "--limit", "3",
            ]))
            .unwrap(),
            Some(CliCommand::Study {
                pool: "Pool111".to_string(),
                limit: Some(3),
            })
        );
    }

    #[test]
    fn parse_oneshot_cycle_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "screen",
                "--wallet",
                "Wallet111",
                "--wallet-sol",
                "0.0",
            ]))
            .unwrap(),
            Some(CliCommand::Screen {
                wallet: Some("Wallet111".to_string()),
                wallet_sol: Some(0.0),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["meridian", "manage", "--wallet", "Wallet111"])).unwrap(),
            Some(CliCommand::Manage {
                wallet: Some("Wallet111".to_string()),
            })
        );
    }

    #[test]
    fn parse_config_get_set_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "config",
                "get",
                "management.deployAmountSol",
                "--file",
                "/tmp/user-config.json",
            ]))
            .unwrap(),
            Some(CliCommand::Config {
                file: Some("/tmp/user-config.json".to_string()),
                action: ConfigAction::Get {
                    key: Some("management.deployAmountSol".to_string()),
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "config",
                "set",
                "management.deployAmountSol",
                "0.25",
                "--file=/tmp/user-config.json",
            ]))
            .unwrap(),
            Some(CliCommand::Config {
                file: Some("/tmp/user-config.json".to_string()),
                action: ConfigAction::Set {
                    key: "management.deployAmountSol".to_string(),
                    value: "0.25".to_string(),
                },
            })
        );
    }

    #[test]
    fn config_get_reads_nested_value() {
        let config = Config::default();
        let output = run_config_command(
            ConfigAction::Get {
                key: Some("management.deployAmountSol".to_string()),
            },
            &config,
            None,
        )
        .expect("config get should read nested value");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "config");
        assert_eq!(parsed["data"]["action"], "get");
        assert_eq!(parsed["data"]["key"], "management.deployAmountSol");
        assert_eq!(parsed["data"]["value"], 0.5);
    }

    #[test]
    fn config_set_updates_nested_value_and_persists_file() {
        let dir = unique_test_dir("config-set");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let path = dir.join("user-config.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&Config::default()).expect("config should serialize"),
        )
        .expect("seed config should be written");

        let output = run_config_command(
            ConfigAction::Set {
                key: "management.deployAmountSol".to_string(),
                value: "0.25".to_string(),
            },
            &Config::default(),
            Some(path.as_path()),
        )
        .expect("config set should persist nested value");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("saved config should be readable"),
        )
        .expect("saved config should remain JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "config");
        assert_eq!(parsed["data"]["action"], "set");
        assert_eq!(parsed["data"]["key"], "management.deployAmountSol");
        assert_eq!(parsed["data"]["value"], 0.25);
        assert_eq!(saved["management"]["deployAmountSol"], 0.25);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_state_intelligence_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "lessons",
                "add",
                "avoid low fee pools",
                "--role",
                "manager",
                "--tags",
                "close,performance",
            ]))
            .unwrap(),
            Some(CliCommand::Lessons {
                action: LessonAction::Add {
                    content: "avoid low fee pools".to_string(),
                    role: Some("manager".to_string()),
                    tags: vec!["close".to_string(), "performance".to_string()],
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "performance",
                "record",
                "--position",
                "pos-1",
                "--pool",
                "pool-1",
                "--symbol",
                "TEST",
                "--pnl",
                "0.05",
                "--fees",
                "0.01",
                "--range-efficiency",
                "0.8",
                "--reason",
                "take_profit",
            ]))
            .unwrap(),
            Some(CliCommand::Performance {
                action: PerformanceAction::Record {
                    position_id: "pos-1".to_string(),
                    pool: "pool-1".to_string(),
                    symbol: "TEST".to_string(),
                    pnl_sol: 0.05,
                    fees_earned: 0.01,
                    range_efficiency: 0.8,
                    close_reason: "take_profit".to_string(),
                    signal_snapshot: "{}".to_string(),
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["meridian", "evolve"])).unwrap(),
            Some(CliCommand::Evolve)
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "pool-memory",
                "add-note",
                "--pool",
                "pool-1",
                "--base-mint",
                "mint-1",
                "--symbol",
                "TEST",
                "--note",
                "watch volatility",
            ]))
            .unwrap(),
            Some(CliCommand::PoolMemory {
                action: PoolMemoryAction::AddNote {
                    pool: "pool-1".to_string(),
                    base_mint: "mint-1".to_string(),
                    symbol: Some("TEST".to_string()),
                    note: "watch volatility".to_string(),
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "blacklist",
                "add",
                "--mint",
                "Mint111",
                "--symbol",
                "SCAM",
                "--reason",
                "rug risk",
            ]))
            .unwrap(),
            Some(CliCommand::Blacklist {
                action: BlacklistAction::Add {
                    mint: "Mint111".to_string(),
                    symbol: Some("SCAM".to_string()),
                    reason: Some("rug risk".to_string()),
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "discord-signals",
                "queue",
                "--pool",
                "Pool111",
                "--base-mint",
                "Mint111",
                "--symbol",
                "DISC",
                "--author",
                "Metlex Pool Bot",
            ]))
            .unwrap(),
            Some(CliCommand::DiscordSignals {
                action: DiscordSignalsAction::Queue {
                    pool: "Pool111".to_string(),
                    base_mint: "Mint111".to_string(),
                    symbol: Some("DISC".to_string()),
                    author: Some("Metlex Pool Bot".to_string()),
                    channel: None,
                    snippet: None,
                },
            })
        );
    }

    #[test]
    fn parse_strategy_library_subcommands() {
        assert_eq!(
            parse_cli_args(&args(&["meridian", "strategies"])).unwrap(),
            Some(CliCommand::Strategies {
                action: StrategyAction::List,
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "strategy",
                "show",
                "custom_ratio_spot"
            ]))
            .unwrap(),
            Some(CliCommand::Strategies {
                action: StrategyAction::Show {
                    id: "custom_ratio_spot".to_string(),
                },
            })
        );
        assert_eq!(
            parse_cli_args(&args(&[
                "meridian",
                "strategies",
                "set-active",
                "multi_layer"
            ]))
            .unwrap(),
            Some(CliCommand::Strategies {
                action: StrategyAction::SetActive {
                    id: "multi_layer".to_string(),
                },
            })
        );
    }

    #[tokio::test]
    async fn strategies_cli_list_creates_isolated_strategy_library() {
        let dir = unique_test_dir("strategies-cli");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let output = run_cli_command(
            CliCommand::Strategies {
                action: StrategyAction::List,
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("strategies list should create default library without network");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        let library_path = dir.join("strategy-library.json");

        assert_eq!(parsed["command"], "strategies");
        assert_eq!(parsed["data"]["action"], "list");
        assert_eq!(parsed["data"]["active"], "custom_ratio_spot");
        assert_eq!(parsed["data"]["count"], 5);
        assert_eq!(parsed["data"]["path"], library_path.display().to_string());
        assert!(library_path.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn discord_signals_cli_queue_list_and_clear_use_isolated_state_dir() {
        let dir = unique_test_dir("discord-signals-cli");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let queue_output = run_cli_command(
            CliCommand::DiscordSignals {
                action: DiscordSignalsAction::Queue {
                    pool: "Pool111".to_string(),
                    base_mint: "Mint111".to_string(),
                    symbol: Some("DISC".to_string()),
                    author: Some("Metlex Pool Bot".to_string()),
                    channel: Some("alpha".to_string()),
                    snippet: Some("new pool".to_string()),
                },
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("discord signal queue should persist without network");
        let queued: serde_json::Value =
            serde_json::from_str(&queue_output.render().unwrap()).expect("queue output JSON");
        let signals_path = dir.join("discord-signals.json");
        assert_eq!(queued["command"], "discord-signals");
        assert_eq!(queued["data"]["action"], "queue");
        assert_eq!(queued["data"]["pending"], 1);
        assert_eq!(queued["data"]["path"], signals_path.display().to_string());
        assert!(signals_path.exists());

        let list_output = run_cli_command(
            CliCommand::DiscordSignals {
                action: DiscordSignalsAction::List,
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("discord signal list should read queue");
        let listed: serde_json::Value =
            serde_json::from_str(&list_output.render().unwrap()).expect("list output JSON");
        assert_eq!(listed["data"]["count"], 1);
        assert_eq!(listed["data"]["signals"][0]["symbol"], "DISC");

        let mut saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&signals_path).expect("signals file should exist"),
        )
        .expect("signals file JSON");
        saved[0]["status"] = serde_json::Value::String("processed".to_string());
        std::fs::write(&signals_path, serde_json::to_string_pretty(&saved).unwrap())
            .expect("signals file should be writable");

        let clear_output = run_cli_command(
            CliCommand::DiscordSignals {
                action: DiscordSignalsAction::Clear,
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("discord signal clear should remove processed signals");
        let cleared: serde_json::Value =
            serde_json::from_str(&clear_output.render().unwrap()).expect("clear output JSON");
        assert_eq!(cleared["data"]["cleared"], 1);
        assert_eq!(cleared["data"]["remaining"], 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn lessons_and_performance_commands_persist_json_state() {
        let dir = unique_test_dir("lessons-performance-cli");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let add_output = run_cli_command(
            CliCommand::Lessons {
                action: LessonAction::Add {
                    content: "avoid low fee pools".to_string(),
                    role: Some("manager".to_string()),
                    tags: vec!["close".to_string()],
                },
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("lesson add should persist");
        let add: serde_json::Value =
            serde_json::from_str(&add_output.render().unwrap()).expect("lesson output JSON");
        assert_eq!(add["command"], "lessons");
        assert_eq!(add["data"]["action"], "add");
        assert_eq!(add["data"]["count"], 1);

        let record_output = run_cli_command(
            CliCommand::Performance {
                action: PerformanceAction::Record {
                    position_id: "pos-1".to_string(),
                    pool: "pool-1".to_string(),
                    symbol: "TEST".to_string(),
                    pnl_sol: 0.05,
                    fees_earned: 0.01,
                    range_efficiency: 0.8,
                    close_reason: "take_profit".to_string(),
                    signal_snapshot: "{}".to_string(),
                },
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("performance record should persist");
        let record: serde_json::Value = serde_json::from_str(&record_output.render().unwrap())
            .expect("performance output JSON");
        assert_eq!(record["command"], "performance");
        assert_eq!(record["data"]["performanceCount"], 1);

        let lessons_path = dir.join("lessons.json");
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&lessons_path).expect("lessons file should exist"),
        )
        .expect("lessons file should be JSON");
        assert_eq!(saved["performance"].as_array().unwrap().len(), 1);
        assert!(saved["lessons"].as_array().unwrap().len() >= 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn pool_memory_blacklist_and_evolve_commands_use_isolated_state_dir() {
        let dir = unique_test_dir("pool-blacklist-cli");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let pool_output = run_cli_command(
            CliCommand::PoolMemory {
                action: PoolMemoryAction::AddNote {
                    pool: "pool-1".to_string(),
                    base_mint: "mint-1".to_string(),
                    symbol: Some("TEST".to_string()),
                    note: "watch volatility".to_string(),
                },
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("pool-memory add-note should persist");
        let pool: serde_json::Value =
            serde_json::from_str(&pool_output.render().unwrap()).expect("pool output JSON");
        assert_eq!(pool["command"], "pool-memory");
        assert_eq!(pool["data"]["poolCount"], 1);

        let blacklist_output = run_cli_command(
            CliCommand::Blacklist {
                action: BlacklistAction::Add {
                    mint: "Mint111".to_string(),
                    symbol: Some("SCAM".to_string()),
                    reason: Some("rug risk".to_string()),
                },
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("blacklist add should persist");
        let blacklist: serde_json::Value =
            serde_json::from_str(&blacklist_output.render().unwrap())
                .expect("blacklist output JSON");
        assert_eq!(blacklist["command"], "blacklist");
        assert_eq!(blacklist["data"]["count"], 1);
        assert!(dir.join("token-blacklist.json").exists());

        let evolve_output = run_cli_command(
            CliCommand::Evolve,
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("evolve should handle no performance data");
        let evolve: serde_json::Value =
            serde_json::from_str(&evolve_output.render().unwrap()).expect("evolve output JSON");
        assert_eq!(evolve["command"], "evolve");
        assert_eq!(evolve["data"]["evolved"], false);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn evolve_command_recalculates_signal_weights_in_isolated_state_dir() {
        let dir = unique_test_dir("darwin-evolve-cli");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();
        let lessons_path = dir.join("lessons.json");

        let mut store = LessonStore::default();
        for idx in 0..10 {
            let is_win = idx < 5;
            let snapshot = if is_win {
                serde_json::json!({
                    "organic_score": 90.0 - idx as f64,
                    "volume": 5000.0
                })
            } else {
                serde_json::json!({
                    "organic_score": 30.0 + idx as f64,
                    "volume": 5000.0
                })
            }
            .to_string();
            store.record_performance(PerformanceInput {
                position_id: &format!("pos-{idx}"),
                pool: &format!("pool-{idx}"),
                symbol: if is_win { "WIN" } else { "LOSS" },
                pnl_sol: if is_win { 0.08 } else { -0.04 },
                fees_earned: 0.01,
                range_efficiency: if is_win { 0.9 } else { 0.3 },
                close_reason: if is_win { "take_profit" } else { "low_yield" },
                signal_snapshot: &snapshot,
            });
        }
        store
            .save(lessons_path.to_str().expect("lessons path should be utf8"))
            .expect("lessons should save");

        let evolve_output = run_cli_command(
            CliCommand::Evolve,
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("evolve should recalculate signal weights");
        let evolve: serde_json::Value =
            serde_json::from_str(&evolve_output.render().unwrap()).expect("evolve output JSON");
        let weights_path = dir.join("signal-weights.json");
        let saved_weights: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&weights_path).expect("signal weights should be saved"),
        )
        .expect("signal weights should be JSON");

        assert_eq!(evolve["command"], "evolve");
        assert!(evolve["data"]["signalWeights"]["changes"]
            .as_array()
            .expect("changes should be an array")
            .iter()
            .any(|change| change["action"] == "boosted"));
        assert!(saved_weights["weights"]["organic_score"].as_f64().unwrap() > 1.0);
        assert_eq!(
            evolve["data"]["signalWeights"]["path"],
            weights_path.display().to_string()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cli_json_envelope_has_success_command_and_data() {
        let output = json_command_output("balance", serde_json::json!({"wallet": "Wallet111"}));
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered output is JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "balance");
        assert_eq!(parsed["data"]["wallet"], "Wallet111");
    }

    #[tokio::test]
    async fn run_cli_deploy_dry_run_uses_json_envelope() {
        let config = Config {
            dry_run: true,
            ..Config::default()
        };
        let output = run_cli_command(
            CliCommand::Deploy {
                pool: "Pool111".to_string(),
                amount_sol: 0.25,
                bins_below: Some(35),
                bins_above: Some(0),
                strategy: Some("spot".to_string()),
                dry_run: true,
            },
            &config,
            "unused-state.json",
        )
        .await
        .expect("dry-run deploy should not need network");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered output is JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "deploy");
        assert_eq!(parsed["data"]["success"], false);
        assert_eq!(parsed["data"]["pool"], "Pool111");
        assert!(parsed["data"]["note"]
            .as_str()
            .expect("dry-run note should be present")
            .contains("DRY RUN"));
    }

    #[tokio::test]
    async fn run_cli_study_dry_run_uses_json_envelope_without_network() {
        let config = Config {
            dry_run: true,
            ..Config::default()
        };
        let output = run_cli_command(
            CliCommand::Study {
                pool: "Pool111".to_string(),
                limit: Some(4),
            },
            &config,
            "unused-state.json",
        )
        .await
        .expect("dry-run study should not need network");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "study");
        assert_eq!(parsed["data"]["pool"], "Pool111");
        assert!(parsed["data"]["lpers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn run_cli_screen_zero_wallet_sol_uses_one_shot_json() {
        let dir = unique_test_dir("screen-oneshot");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let output = run_cli_command(
            CliCommand::Screen {
                wallet: Some("Wallet111".to_string()),
                wallet_sol: Some(0.0),
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("zero-balance screen should not call LLM or network");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "screen");
        assert!(parsed["data"]["result"]
            .as_str()
            .expect("screen result should be present")
            .contains("Not enough SOL"));
        assert_eq!(parsed["data"]["activePositions"], 0);
        assert_eq!(parsed["data"]["walletSol"], 0.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_cli_manage_no_positions_uses_one_shot_json() {
        let dir = unique_test_dir("manage-oneshot");
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let state_path = dir.join("meridian-state.json");
        let config = Config::default();

        let output = run_cli_command(
            CliCommand::Manage {
                wallet: Some("Wallet111".to_string()),
            },
            &config,
            state_path.to_str().expect("state path should be utf8"),
        )
        .await
        .expect("empty-state manage should not call LLM or network");
        let rendered = output.render().expect("json output should render");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["command"], "manage");
        assert_eq!(parsed["data"]["result"], "No active positions.");
        assert_eq!(parsed["data"]["activePositions"], 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn json_parity_commands_have_stable_command_names() {
        let cases = vec![
            (CliCommand::Balance { wallet: None }, "balance"),
            (CliCommand::Positions { wallet: None }, "positions"),
            (
                CliCommand::Pnl {
                    pool: "Pool111".to_string(),
                    position: "Pos111".to_string(),
                    wallet: None,
                },
                "pnl",
            ),
            (CliCommand::Candidates { limit: None }, "candidates"),
            (
                CliCommand::Study {
                    pool: "Pool111".to_string(),
                    limit: None,
                },
                "study",
            ),
            (
                CliCommand::Deploy {
                    pool: "Pool111".to_string(),
                    amount_sol: 0.25,
                    bins_below: None,
                    bins_above: None,
                    strategy: None,
                    dry_run: true,
                },
                "deploy",
            ),
            (
                CliCommand::Claim {
                    position: "Pos111".to_string(),
                },
                "claim",
            ),
            (
                CliCommand::Close {
                    position: "Pos111".to_string(),
                    reason: None,
                    skip_swap: true,
                },
                "close",
            ),
            (
                CliCommand::Swap {
                    mint: "Mint111".to_string(),
                    amount: 1.0,
                },
                "swap",
            ),
            (
                CliCommand::Screen {
                    wallet: None,
                    wallet_sol: Some(0.0),
                },
                "screen",
            ),
            (CliCommand::Manage { wallet: None }, "manage"),
        ];

        for (command, expected_name) in cases {
            assert_eq!(command_name(&command), expected_name);
        }
    }
}
