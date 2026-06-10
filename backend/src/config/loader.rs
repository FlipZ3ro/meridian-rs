use super::types::Config;
use anyhow::Result;
use serde_json::{Map, Value};
use std::path::PathBuf;

fn find_config_path(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    // ~/.meridian/user-config.json > ./user-config.json
    let home_config = dirs_home().join("user-config.json");
    if home_config.exists() {
        return home_config;
    }
    PathBuf::from("user-config.json")
}

fn dirs_home() -> PathBuf {
    if let Ok(h) = std::env::var("MERIDIAN_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".meridian")
}

pub fn load_env_files() {
    // Match the original JS project convention: allow repo-local `.env` for app
    // runs and `~/.meridian/.env` for globally installed CLI/runtime usage.
    let home_env = dirs_home().join(".env");
    if home_env.exists() {
        let _ = dotenvy::from_path(&home_env);
    }
    let repo_env = PathBuf::from(".env");
    if repo_env.exists() {
        let _ = dotenvy::from_path(&repo_env);
    }
}

pub fn meridian_data_path(file_name: &str) -> PathBuf {
    let dir = std::env::var("MERIDIAN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home());
    let _ = std::fs::create_dir_all(&dir);
    dir.join(file_name)
}

pub fn resolve_config_path(explicit: Option<&str>) -> PathBuf {
    find_config_path(explicit)
}

pub fn load_config(path: Option<&str>) -> Result<Config> {
    let config_path = find_config_path(path);
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let mut config = parse_config(&content)?;
        apply_env_overrides(&mut config);
        apply_runtime_env(&config);
        tracing::info!("Loaded config from {}", config_path.display());
        Ok(config)
    } else {
        tracing::warn!(
            "No config file found at {}, using defaults",
            config_path.display()
        );
        let mut config = Config::default();
        apply_env_overrides(&mut config);
        apply_runtime_env(&config);
        Ok(config)
    }
}

fn parse_config(content: &str) -> Result<Config> {
    let value: Value = serde_json::from_str(content)?;
    if is_nested_config(&value) {
        Ok(serde_json::from_value(value)?)
    } else {
        Ok(config_from_flat_js_value(&value))
    }
}

fn is_nested_config(value: &Value) -> bool {
    value.get("screening").is_some()
        || value.get("management").is_some()
        || value.get("risk").is_some()
        || value.get("schedule").is_some()
        || value.get("llm").is_some()
}

fn config_from_flat_js_value(value: &Value) -> Config {
    let mut config = Config::default();
    let Some(obj) = value.as_object() else {
        return config;
    };

    // Original Node.js Meridian uses a flat user-config.json. Keep Rust tolerant so
    // users can copy that file directly while the Rust port keeps its nested types.
    set_bool(obj, "dryRun", &mut config.dry_run);
    set_f64(
        obj,
        "minFeeActiveTvlRatio",
        &mut config.screening.min_fee_active_tvl_ratio,
    );
    set_f64(obj, "minTvl", &mut config.screening.min_tvl);
    set_opt_f64(obj, "maxTvl", &mut config.screening.max_tvl);
    set_f64(obj, "minVolume", &mut config.screening.min_volume);
    set_f64(obj, "minOrganic", &mut config.screening.min_organic);
    set_f64(
        obj,
        "minQuoteOrganic",
        &mut config.screening.min_quote_organic,
    );
    set_u64(obj, "minHolders", &mut config.screening.min_holders);
    set_f64(obj, "minMcap", &mut config.screening.min_mcap);
    set_f64(obj, "maxMcap", &mut config.screening.max_mcap);
    set_u16(obj, "minBinStep", &mut config.screening.min_bin_step);
    set_u16(obj, "maxBinStep", &mut config.screening.max_bin_step);
    set_string(obj, "timeframe", &mut config.screening.timeframe);
    set_string(obj, "category", &mut config.screening.category);
    set_f64(
        obj,
        "minTokenFeesSol",
        &mut config.screening.min_token_fees_sol,
    );
    set_f64(
        obj,
        "maxBotHoldersPct",
        &mut config.screening.max_bot_holders_pct,
    );
    set_opt_f64(
        obj,
        "maxBundlersPct",
        &mut config.screening.max_bundlers_pct,
    );
    set_f64(obj, "maxTop10Pct", &mut config.screening.max_top10_pct);
    set_vec_string(
        obj,
        "blockedLaunchpads",
        &mut config.screening.blocked_launchpads,
    );
    set_vec_string(
        obj,
        "allowedLaunchpads",
        &mut config.screening.allowed_launchpads,
    );
    set_bool(
        obj,
        "excludeHighSupplyConcentration",
        &mut config.screening.exclude_high_supply_concentration,
    );
    set_opt_f64(
        obj,
        "minTokenAgeHours",
        &mut config.screening.min_token_age_hours,
    );
    set_opt_f64(
        obj,
        "maxTokenAgeHours",
        &mut config.screening.max_token_age_hours,
    );
    set_bool(
        obj,
        "useDiscordSignals",
        &mut config.screening.use_discord_signals,
    );
    set_opt_string(
        obj,
        "discordSignalMode",
        &mut config.screening.discord_signal_mode,
    );
    set_bool(
        obj,
        "avoidPvpSymbols",
        &mut config.screening.avoid_pvp_symbols,
    );
    set_bool(
        obj,
        "blockPvpSymbols",
        &mut config.screening.block_pvp_symbols,
    );

    set_f64(
        obj,
        "deployAmountSol",
        &mut config.management.deploy_amount_sol,
    );
    set_f64(obj, "gasReserve", &mut config.management.gas_reserve);
    set_f64(
        obj,
        "positionSizePct",
        &mut config.management.position_size_pct,
    );
    set_f64(obj, "minSolToOpen", &mut config.management.min_sol_to_open);
    set_u32(
        obj,
        "outOfRangeWaitMinutes",
        &mut config.management.out_of_range_wait_minutes,
    );
    set_u32(
        obj,
        "oorCooldownTriggerCount",
        &mut config.management.oor_cooldown_trigger_count,
    );
    set_u32(
        obj,
        "oorCooldownHours",
        &mut config.management.oor_cooldown_hours,
    );
    set_bool(
        obj,
        "repeatDeployCooldownEnabled",
        &mut config.management.repeat_deploy_cooldown_enabled,
    );
    set_u32(
        obj,
        "repeatDeployCooldownTriggerCount",
        &mut config.management.repeat_deploy_cooldown_trigger_count,
    );
    set_u32(
        obj,
        "repeatDeployCooldownHours",
        &mut config.management.repeat_deploy_cooldown_hours,
    );
    set_string(
        obj,
        "repeatDeployCooldownScope",
        &mut config.management.repeat_deploy_cooldown_scope,
    );
    set_f64(
        obj,
        "repeatDeployCooldownMinFeeEarnedPct",
        &mut config.management.repeat_deploy_cooldown_min_fee_earned_pct,
    );
    set_opt_f64(obj, "takeProfitPct", &mut config.management.take_profit_pct);
    set_u32(
        obj,
        "managementIntervalMin",
        &mut config.management.management_interval_min,
    );
    set_u32(
        obj,
        "screeningIntervalMin",
        &mut config.management.screening_interval_min,
    );
    set_bool(
        obj,
        "trailingTakeProfit",
        &mut config.management.trailing_take_profit,
    );
    set_f64(
        obj,
        "trailingTriggerPct",
        &mut config.management.trailing_trigger_pct,
    );
    set_f64(
        obj,
        "trailingDropPct",
        &mut config.management.trailing_drop_pct,
    );
    set_f64(
        obj,
        "minClaimAmount",
        &mut config.management.min_claim_amount,
    );
    set_f64(
        obj,
        "minFeePerTvl24h",
        &mut config.management.min_fee_per_tvl_24h,
    );
    set_u32(
        obj,
        "minAgeBeforeYieldCheck",
        &mut config.management.min_age_before_yield_check,
    );
    set_i32(
        obj,
        "outOfRangeBinsToClose",
        &mut config.management.out_of_range_bins_to_close,
    );
    set_bool(obj, "solMode", &mut config.management.sol_mode);

    set_f64(obj, "maxDeployAmount", &mut config.risk.max_deploy_amount);
    set_u32(obj, "maxPositions", &mut config.risk.max_positions);
    set_opt_f64(obj, "stopLossPct", &mut config.risk.stop_loss_pct);

    set_u32(
        obj,
        "managementIntervalMin",
        &mut config.schedule.management_interval_min,
    );
    set_u32(
        obj,
        "screeningIntervalMin",
        &mut config.schedule.screening_interval_min,
    );

    if let Some(model) = non_empty_string(obj, "llmModel") {
        config.llm.management_model = model.clone();
        config.llm.screening_model = model.clone();
        config.llm.general_model = model;
    }
    set_string(obj, "managementModel", &mut config.llm.management_model);
    set_string(obj, "screeningModel", &mut config.llm.screening_model);
    set_string(obj, "generalModel", &mut config.llm.general_model);
    set_string(obj, "llmBaseUrl", &mut config.llm.base_url);
    set_opt_string(obj, "llmApiKey", &mut config.llm.api_key);
    set_f32(obj, "temperature", &mut config.llm.temperature);
    set_u32(obj, "maxTokens", &mut config.llm.max_tokens);
    set_u32(obj, "maxSteps", &mut config.llm.max_steps);

    set_bool(obj, "darwinEnabled", &mut config.darwin.enabled);
    set_u64(obj, "darwinWindowDays", &mut config.darwin.window_days);
    set_u64(obj, "darwinRecalcEvery", &mut config.darwin.recalc_every);
    set_f64(obj, "darwinBoost", &mut config.darwin.boost_factor);
    set_f64(obj, "darwinDecay", &mut config.darwin.decay_factor);
    set_f64(obj, "darwinFloor", &mut config.darwin.weight_floor);
    set_f64(obj, "darwinCeiling", &mut config.darwin.weight_ceiling);
    set_u64(obj, "darwinMinSamples", &mut config.darwin.min_samples);

    set_u32(obj, "minBinsBelow", &mut config.strategy.min_bins_below);
    set_u32(obj, "maxBinsBelow", &mut config.strategy.max_bins_below);
    set_u32(
        obj,
        "minSafeBinsBelow",
        &mut config.strategy.min_safe_bins_below,
    );
    if config.strategy.max_bins_below < config.strategy.min_bins_below {
        config.strategy.max_bins_below = config.strategy.min_bins_below;
    }

    set_opt_string(obj, "rpcUrl", &mut config.api.helius_rpc_url);
    set_opt_string(obj, "heliusApiKey", &mut config.api.helius_api_key);
    set_opt_string(
        obj,
        "agentMeridianApiUrl",
        &mut config.api.agent_meridian_base,
    );
    set_opt_string(obj, "publicApiKey", &mut config.api.agent_meridian_key);
    set_bool(
        obj,
        "lpAgentRelayEnabled",
        &mut config.api.lp_agent_relay_enabled,
    );
    set_opt_string(obj, "telegramBotToken", &mut config.api.telegram_bot_token);
    set_opt_string(obj, "telegramChatId", &mut config.api.telegram_chat_id);

    if let Some(indicators) = obj.get("chartIndicators").and_then(Value::as_object) {
        set_bool(indicators, "enabled", &mut config.indicators.enabled);
        set_opt_string(
            indicators,
            "entryPreset",
            &mut config.indicators.entry_preset,
        );
        set_opt_string(indicators, "exitPreset", &mut config.indicators.exit_preset);
        set_u32(indicators, "rsiLength", &mut config.indicators.rsi_length);
        set_vec_string(indicators, "intervals", &mut config.indicators.intervals);
        set_u32(indicators, "candles", &mut config.indicators.candles);
        set_f64(
            indicators,
            "rsiOversold",
            &mut config.indicators.rsi_oversold,
        );
        set_f64(
            indicators,
            "rsiOverbought",
            &mut config.indicators.rsi_overbought,
        );
        set_bool(
            indicators,
            "requireAllIntervals",
            &mut config.indicators.require_all_intervals,
        );

        let mut presets = Vec::new();
        if let Some(entry) = config.indicators.entry_preset.clone() {
            presets.push(entry);
        }
        if let Some(exit) = config.indicators.exit_preset.clone() {
            if !presets.contains(&exit) {
                presets.push(exit);
            }
        }
        if !presets.is_empty() {
            config.indicators.presets = presets;
        }
    }

    config
}

fn apply_env_overrides(config: &mut Config) {
    if let Some(dry_run) = env_bool("DRY_RUN") {
        config.dry_run = dry_run;
    }
    if let Some(rpc_url) = env_non_empty("RPC_URL").or_else(|| env_non_empty("HELIUS_RPC_URL")) {
        config.api.helius_rpc_url = Some(rpc_url);
    }
    if let Some(helius_key) = env_non_empty("HELIUS_API_KEY") {
        config.api.helius_api_key = Some(helius_key);
    }
    if let Some(base_url) = env_non_empty("LLM_BASE_URL") {
        config.llm.base_url = base_url;
    }
    if let Some(api_key) =
        env_non_empty("LLM_API_KEY").or_else(|| env_non_empty("OPENROUTER_API_KEY"))
    {
        config.llm.api_key = Some(api_key);
    }
    if let Some(model) = env_non_empty("LLM_MODEL") {
        config.llm.management_model = model.clone();
        config.llm.screening_model = model.clone();
        config.llm.general_model = model;
    }
    if let Some(model) = env_non_empty("MANAGEMENT_MODEL") {
        config.llm.management_model = model;
    }
    if let Some(model) = env_non_empty("SCREENING_MODEL") {
        config.llm.screening_model = model;
    }
    if let Some(model) = env_non_empty("GENERAL_MODEL") {
        config.llm.general_model = model;
    }
    if let Some(token) = env_non_empty("TELEGRAM_BOT_TOKEN") {
        config.api.telegram_bot_token = Some(token);
    }
    if let Some(chat_id) = env_non_empty("TELEGRAM_CHAT_ID") {
        config.api.telegram_chat_id = Some(chat_id);
    }
    if let Some(base) = env_non_empty("AGENT_MERIDIAN_API_URL") {
        config.api.agent_meridian_base = Some(base);
    }
    if let Some(key) = env_non_empty("PUBLIC_API_KEY").or_else(|| env_non_empty("LPAGENT_API_KEY"))
    {
        config.api.agent_meridian_key = Some(key);
    }
    if let Some(key) = env_non_empty("JUPITER_API_KEY") {
        config.jupiter.api_key = Some(key);
    }
    if let Some(account) = env_non_empty("JUPITER_REFERRAL_ACCOUNT") {
        config.jupiter.referral_account = Some(account);
    }
    if let Ok(fee_bps) = std::env::var("JUPITER_REFERRAL_FEE_BPS") {
        if let Ok(parsed) = fee_bps.trim().parse::<u32>() {
            config.jupiter.referral_fee_bps = parsed;
        }
    }
}

fn apply_runtime_env(config: &Config) {
    std::env::set_var("DRY_RUN", if config.dry_run { "true" } else { "false" });
}

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn non_empty_string(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn number_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn number_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn number_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn set_f64(obj: &Map<String, Value>, key: &str, target: &mut f64) {
    if let Some(value) = obj.get(key).and_then(number_as_f64) {
        *target = value;
    }
}

fn set_f32(obj: &Map<String, Value>, key: &str, target: &mut f32) {
    if let Some(value) = obj.get(key).and_then(number_as_f64) {
        *target = value as f32;
    }
}

fn set_opt_f64(obj: &Map<String, Value>, key: &str, target: &mut Option<f64>) {
    if let Some(value) = obj.get(key) {
        *target = if value.is_null() {
            None
        } else {
            number_as_f64(value)
        };
    }
}

fn set_u64(obj: &Map<String, Value>, key: &str, target: &mut u64) {
    if let Some(value) = obj.get(key).and_then(number_as_u64) {
        *target = value;
    }
}

fn set_u32(obj: &Map<String, Value>, key: &str, target: &mut u32) {
    if let Some(value) = obj
        .get(key)
        .and_then(number_as_u64)
        .and_then(|v| u32::try_from(v).ok())
    {
        *target = value;
    }
}

fn set_u16(obj: &Map<String, Value>, key: &str, target: &mut u16) {
    if let Some(value) = obj
        .get(key)
        .and_then(number_as_u64)
        .and_then(|v| u16::try_from(v).ok())
    {
        *target = value;
    }
}

fn set_i32(obj: &Map<String, Value>, key: &str, target: &mut i32) {
    if let Some(value) = obj
        .get(key)
        .and_then(number_as_i64)
        .and_then(|v| i32::try_from(v).ok())
    {
        *target = value;
    }
}

fn set_bool(obj: &Map<String, Value>, key: &str, target: &mut bool) {
    if let Some(value) = obj.get(key) {
        if let Some(parsed) = value.as_bool().or_else(|| {
            value
                .as_str()
                .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Some(true),
                    "false" | "0" | "no" => Some(false),
                    _ => None,
                })
        }) {
            *target = parsed;
        }
    }
}

fn set_string(obj: &Map<String, Value>, key: &str, target: &mut String) {
    if let Some(value) = non_empty_string(obj, key) {
        *target = value;
    }
}

fn set_opt_string(obj: &Map<String, Value>, key: &str, target: &mut Option<String>) {
    if obj.get(key).is_some() {
        *target = non_empty_string(obj, key);
    }
}

fn set_vec_string(obj: &Map<String, Value>, key: &str, target: &mut Vec<String>) {
    if let Some(values) = obj.get(key).and_then(Value::as_array) {
        *target = values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }
}

pub fn save_config(config: &Config, path: Option<&str>) -> Result<()> {
    let config_path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| find_config_path(None));
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&config_path, content)?;
    Ok(())
}

/// Compute deploy amount based on wallet SOL balance
pub fn compute_deploy_amount(config: &Config, wallet_sol: f64) -> f64 {
    let by_pct = wallet_sol * config.management.position_size_pct;
    let amount = by_pct
        .min(config.risk.max_deploy_amount)
        .max(config.management.deploy_amount_sol);
    if wallet_sol - amount < config.management.gas_reserve {
        return 0.0; // not enough SOL after gas reserve
    }
    amount
}

#[cfg(test)]
mod tests {
    use super::{load_config, meridian_data_path};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ENV_KEYS: &[&str] = &[
        "DRY_RUN",
        "MERIDIAN_DATA_DIR",
        "RPC_URL",
        "HELIUS_RPC_URL",
        "HELIUS_API_KEY",
        "LLM_BASE_URL",
        "LLM_API_KEY",
        "OPENROUTER_API_KEY",
        "LLM_MODEL",
        "MANAGEMENT_MODEL",
        "SCREENING_MODEL",
        "GENERAL_MODEL",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "AGENT_MERIDIAN_API_URL",
        "PUBLIC_API_KEY",
        "LPAGENT_API_KEY",
        "JUPITER_API_KEY",
        "JUPITER_REFERRAL_ACCOUNT",
        "JUPITER_REFERRAL_FEE_BPS",
    ];

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn clear(keys: &'static [&'static str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for key in keys {
                std::env::remove_var(key);
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn temp_config_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "meridian-rs-{}-{}-{}.json",
            label,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn meridian_data_path_uses_isolated_data_dir() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(ENV_KEYS);
        let data_dir = std::env::temp_dir().join(format!(
            "meridian-rs-data-dir-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::env::set_var("MERIDIAN_DATA_DIR", &data_dir);

        let path = meridian_data_path("meridian-state.json");

        assert_eq!(path, data_dir.join("meridian-state.json"));
        assert!(data_dir.exists());
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn load_config_accepts_original_js_flat_user_config() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(ENV_KEYS);
        let path = temp_config_path("flat-config");
        let raw = r#"
        {
          "dryRun": true,
          "rpcUrl": "https://pump.helius-rpc.com",
          "llmBaseUrl": "https://openrouter.ai/api/v1",
          "llmApiKey": "sk-test",
          "llmModel": "minimax/minimax-m2.7",
          "deployAmountSol": 0.7,
          "maxPositions": 4,
          "minSolToOpen": 0.9,
          "maxDeployAmount": 42,
          "gasReserve": 0.3,
          "positionSizePct": 0.25,
          "minBinsBelow": 35,
          "maxBinsBelow": 69,
          "timeframe": "5m",
          "category": "trending",
          "excludeHighSupplyConcentration": true,
          "minTvl": 10000,
          "maxTvl": 150000,
          "minVolume": 500,
          "minOrganic": 60,
          "minQuoteOrganic": 60,
          "minHolders": 500,
          "minMcap": 150000,
          "maxMcap": 10000000,
          "minBinStep": 80,
          "maxBinStep": 125,
          "minFeeActiveTvlRatio": 0.05,
          "minTokenFeesSol": 30,
          "maxBotHoldersPct": 30,
          "maxTop10Pct": 60,
          "blockedLaunchpads": ["pump.fun"],
          "allowedLaunchpads": ["meteora"],
          "useDiscordSignals": true,
          "discordSignalMode": "only",
          "avoidPvpSymbols": false,
          "blockPvpSymbols": true,
          "minClaimAmount": 5,
          "outOfRangeBinsToClose": 10,
          "outOfRangeWaitMinutes": 30,
          "stopLossPct": -50,
          "takeProfitPct": 5,
          "minFeePerTvl24h": 7,
          "minAgeBeforeYieldCheck": 60,
          "oorCooldownTriggerCount": 2,
          "oorCooldownHours": 8,
          "repeatDeployCooldownEnabled": true,
          "repeatDeployCooldownTriggerCount": 4,
          "repeatDeployCooldownHours": 9,
          "repeatDeployCooldownScope": "both",
          "repeatDeployCooldownMinFeeEarnedPct": 0.25,
          "trailingTakeProfit": true,
          "trailingTriggerPct": 3,
          "trailingDropPct": 1.5,
          "solMode": true,
          "managementIntervalMin": 10,
          "screeningIntervalMin": 30,
          "temperature": 0.373,
          "maxTokens": 4096,
          "maxSteps": 20,
          "managementModel": "minimax/minimax-m2.5",
          "screeningModel": "minimax/minimax-m2.5",
          "generalModel": "minimax/minimax-m2.7",
          "darwinEnabled": true,
          "darwinWindowDays": 30,
          "darwinRecalcEvery": 4,
          "darwinBoost": 1.11,
          "darwinDecay": 0.91,
          "darwinFloor": 0.25,
          "darwinCeiling": 2.75,
          "darwinMinSamples": 6,
          "agentMeridianApiUrl": "https://api.agentmeridian.xyz/api",
          "publicApiKey": "public-test",
          "lpAgentRelayEnabled": true,
          "telegramChatId": "12345",
          "chartIndicators": {
            "enabled": true,
            "entryPreset": "supertrend_break",
            "exitPreset": "rsi_reversal",
            "rsiLength": 3,
            "intervals": ["5_MINUTE", "15_MINUTE"],
            "candles": 199,
            "rsiOversold": 25,
            "rsiOverbought": 75,
            "requireAllIntervals": true
          }
        }
        "#;

        std::fs::write(&path, raw).expect("write flat config fixture");
        let config = load_config(Some(path.to_str().expect("utf8 temp path")))
            .expect("original JS flat config should load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(config.management.deploy_amount_sol, 0.7);
        assert!(config.dry_run);
        assert_eq!(std::env::var("DRY_RUN").as_deref(), Ok("true"));
        assert_eq!(config.risk.max_positions, 4);
        assert_eq!(config.risk.max_deploy_amount, 42.0);
        assert_eq!(config.screening.min_quote_organic, 60.0);
        assert_eq!(config.screening.blocked_launchpads, vec!["pump.fun"]);
        assert_eq!(config.screening.allowed_launchpads, vec!["meteora"]);
        assert!(config.screening.use_discord_signals);
        assert_eq!(
            config.screening.discord_signal_mode.as_deref(),
            Some("only")
        );
        assert!(!config.screening.avoid_pvp_symbols);
        assert!(config.screening.block_pvp_symbols);
        assert_eq!(config.strategy.min_bins_below, 35);
        assert_eq!(config.strategy.max_bins_below, 69);
        assert_eq!(config.llm.management_model, "minimax/minimax-m2.5");
        assert_eq!(config.llm.screening_model, "minimax/minimax-m2.5");
        assert_eq!(config.llm.general_model, "minimax/minimax-m2.7");
        assert_eq!(config.management.min_age_before_yield_check, 60);
        assert_eq!(config.management.oor_cooldown_trigger_count, 2);
        assert_eq!(config.management.oor_cooldown_hours, 8);
        assert!(config.management.repeat_deploy_cooldown_enabled);
        assert_eq!(config.management.repeat_deploy_cooldown_trigger_count, 4);
        assert_eq!(config.management.repeat_deploy_cooldown_hours, 9);
        assert_eq!(config.management.repeat_deploy_cooldown_scope, "both");
        assert_eq!(
            config.management.repeat_deploy_cooldown_min_fee_earned_pct,
            0.25
        );
        assert!(config.darwin.enabled);
        assert_eq!(config.darwin.window_days, 30);
        assert_eq!(config.darwin.recalc_every, 4);
        assert_eq!(config.darwin.boost_factor, 1.11);
        assert_eq!(config.darwin.decay_factor, 0.91);
        assert_eq!(config.darwin.weight_floor, 0.25);
        assert_eq!(config.darwin.weight_ceiling, 2.75);
        assert_eq!(config.darwin.min_samples, 6);
        assert_eq!(config.llm.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(config.llm.api_key.as_deref(), Some("sk-test"));
        assert_eq!(
            config.api.helius_rpc_url.as_deref(),
            Some("https://pump.helius-rpc.com")
        );
        assert_eq!(
            config.api.agent_meridian_base.as_deref(),
            Some("https://api.agentmeridian.xyz/api")
        );
        assert_eq!(
            config.api.agent_meridian_key.as_deref(),
            Some("public-test")
        );
        assert!(config.api.lp_agent_relay_enabled);
        assert_eq!(config.api.telegram_chat_id.as_deref(), Some("12345"));
        assert!(config.indicators.enabled);
        assert_eq!(
            config.indicators.entry_preset.as_deref(),
            Some("supertrend_break")
        );
        assert_eq!(
            config.indicators.exit_preset.as_deref(),
            Some("rsi_reversal")
        );
        assert_eq!(
            config.indicators.presets,
            vec!["supertrend_break", "rsi_reversal"]
        );
        assert_eq!(config.indicators.rsi_length, 3);
        assert_eq!(config.indicators.intervals, vec!["5_MINUTE", "15_MINUTE"]);
        assert_eq!(config.indicators.candles, 199);
        assert_eq!(config.indicators.rsi_oversold, 25.0);
        assert_eq!(config.indicators.rsi_overbought, 75.0);
        assert!(config.indicators.require_all_intervals);
    }

    #[test]
    fn load_config_applies_original_js_env_aliases() {
        let _lock = ENV_LOCK.lock().expect("env test lock");
        let _env = EnvGuard::clear(ENV_KEYS);
        std::env::set_var("RPC_URL", "https://rpc.example.test");
        std::env::set_var("HELIUS_API_KEY", "helius-test");
        std::env::set_var("OPENROUTER_API_KEY", "openrouter-test");
        std::env::set_var("LLM_BASE_URL", "https://llm.example.test/v1");
        std::env::set_var("LLM_MODEL", "openrouter/test-model");
        std::env::set_var("MANAGEMENT_MODEL", "openrouter/management-model");
        std::env::set_var("SCREENING_MODEL", "openrouter/screening-model");
        std::env::set_var("GENERAL_MODEL", "openrouter/general-model");
        std::env::set_var("TELEGRAM_BOT_TOKEN", "telegram-token");
        std::env::set_var("TELEGRAM_CHAT_ID", "telegram-chat");
        std::env::set_var("AGENT_MERIDIAN_API_URL", "https://agent.example.test/api");
        std::env::set_var("LPAGENT_API_KEY", "lpagent-test");
        std::env::set_var("JUPITER_API_KEY", "jupiter-test");
        std::env::set_var("JUPITER_REFERRAL_ACCOUNT", "referral-test");
        std::env::set_var("JUPITER_REFERRAL_FEE_BPS", "25");

        let path = temp_config_path("env-aliases");
        std::fs::write(&path, "{}").expect("write config fixture");
        let config = load_config(Some(path.to_str().expect("utf8 temp path")))
            .expect("config with original env aliases should load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            config.api.helius_rpc_url.as_deref(),
            Some("https://rpc.example.test")
        );
        assert_eq!(config.api.helius_api_key.as_deref(), Some("helius-test"));
        assert_eq!(config.llm.api_key.as_deref(), Some("openrouter-test"));
        assert_eq!(config.llm.base_url, "https://llm.example.test/v1");
        assert_eq!(config.llm.management_model, "openrouter/management-model");
        assert_eq!(config.llm.screening_model, "openrouter/screening-model");
        assert_eq!(config.llm.general_model, "openrouter/general-model");
        assert_eq!(
            config.api.telegram_bot_token.as_deref(),
            Some("telegram-token")
        );
        assert_eq!(
            config.api.telegram_chat_id.as_deref(),
            Some("telegram-chat")
        );
        assert_eq!(
            config.api.agent_meridian_base.as_deref(),
            Some("https://agent.example.test/api")
        );
        assert_eq!(
            config.api.agent_meridian_key.as_deref(),
            Some("lpagent-test")
        );
        assert_eq!(config.jupiter.api_key.as_deref(), Some("jupiter-test"));
        assert_eq!(
            config.jupiter.referral_account.as_deref(),
            Some("referral-test")
        );
        assert_eq!(config.jupiter.referral_fee_bps, 25);
    }
}
