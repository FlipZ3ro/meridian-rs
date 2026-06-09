use super::types::*;

/// Hardcoded VPS config — no user-config.json needed.
/// Sensitive values (RPC, API keys, wallet) still come from env vars.
pub fn vps_config() -> Config {
    Config {
        dry_run: true,

        screening: ScreeningConfig {
            min_fee_active_tvl_ratio: 0.2,
            min_tvl: 10_000.0,
            max_tvl: Some(150_000.0),
            min_volume: 10_000.0,
            min_organic: 60.0,
            min_quote_organic: 60.0,
            min_holders: 500,
            min_mcap: 150_000.0,
            max_mcap: 10_000_000.0,
            min_bin_step: 100,
            max_bin_step: 125,
            timeframe: "1h".to_string(),
            category: "trending".to_string(),
            min_token_fees_sol: 30.0,
            max_bot_holders_pct: 30.0,
            max_bundlers_pct: Some(30.0),
            max_top10_pct: 60.0,
            blocked_launchpads: vec![],
            allowed_launchpads: vec![],
            exclude_high_supply_concentration: true,
            min_token_age_hours: None,
            max_token_age_hours: None,
            use_discord_signals: false,
            discord_signal_mode: Some("merge".to_string()),
            avoid_pvp_symbols: true,
            block_pvp_symbols: false,
        },

        management: ManagementConfig {
            deploy_amount_sol: 0.5,
            gas_reserve: 0.2,
            position_size_pct: 0.35,
            min_sol_to_open: 0.55,
            out_of_range_wait_minutes: 30,
            oor_cooldown_trigger_count: 2,
            oor_cooldown_hours: 8,
            repeat_deploy_cooldown_enabled: true,
            repeat_deploy_cooldown_trigger_count: 4,
            repeat_deploy_cooldown_hours: 9,
            repeat_deploy_cooldown_scope: "both".to_string(),
            repeat_deploy_cooldown_min_fee_earned_pct: 0.25,
            take_profit_pct: None,
            management_interval_min: 10,
            screening_interval_min: 30,
            trailing_take_profit: true,
            trailing_trigger_pct: 3.0,
            trailing_drop_pct: 1.5,
            min_claim_amount: 5.0,
            min_fee_per_tvl_24h: 7.0,
            min_age_before_yield_check: 60,
            out_of_range_bins_to_close: 10,
            sol_mode: true,
        },

        risk: RiskConfig {
            max_deploy_amount: 50.0,
            max_positions: 3,
            stop_loss_pct: Some(-50.0),
            cooldown_loss_pct: -5.0,
            cooldown_duration_min: 60,
        },

        schedule: ScheduleConfig {
            management_interval_min: 10,
            screening_interval_min: 30,
            pnl_poll_interval_secs: 30,
            sync_interval_min: 5,
        },

        llm: LlmConfig {
            management_model: "minimax/minimax-m2.5".to_string(),
            screening_model: "minimax/minimax-m2.5".to_string(),
            general_model: "minimax/minimax-m2.7".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: None, // from OPENROUTER_API_KEY or LLM_API_KEY env
            temperature: 0.373,
            max_tokens: 4096,
            max_steps: 20,
        },

        strategy: StrategyConfig {
            min_bins_below: 35,
            max_bins_below: 69,
            min_safe_bins_below: 35,
        },

        dual_strategy: DualStrategyConfig {
            enabled: false,
            primary_pct: 0.6,
            safeguard_oor_wait_min: 60,
            aggressive_oor_wait_min: 15,
        },

        tokens: TokensConfig::default(),

        api: ApiConfig {
            helius_rpc_url: None, // from RPC_URL env
            helius_api_key: None, // from HELIUS_API_KEY env
            agent_meridian_base: Some("https://api.agentmeridian.xyz/api".to_string()),
            agent_meridian_key: None, // from PUBLIC_API_KEY env
            lp_agent_relay_enabled: false,
            telegram_bot_token: None, // from TELEGRAM_BOT_TOKEN env
            telegram_chat_id: None,   // from TELEGRAM_CHAT_ID env
        },

        jupiter: JupiterConfig {
            api_key: None,          // from JUPITER_API_KEY env
            referral_account: None, // from JUPITER_REFERRAL_ACCOUNT env
            referral_fee_bps: 25,
        },

        indicators: IndicatorsConfig {
            enabled: true,
            entry_preset: Some("supertrend_break".to_string()),
            exit_preset: Some("rsi_reversal".to_string()),
            rsi_length: 3,
            intervals: vec!["5_MINUTE".to_string(), "15_MINUTE".to_string()],
            candles: 199,
            rsi_oversold: 25.0,
            rsi_overbought: 75.0,
            require_all_intervals: true,
            presets: vec!["supertrend_break".to_string(), "rsi_reversal".to_string()],
        },

        darwin: DarwinConfig {
            enabled: true,
            window_days: 30,
            recalc_every: 4,
            boost_factor: 1.11,
            decay_factor: 0.91,
            weight_floor: 0.25,
            weight_ceiling: 2.75,
            min_samples: 6,
        },
    }
}
