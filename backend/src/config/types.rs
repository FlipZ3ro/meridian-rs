use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub screening: ScreeningConfig,
    pub management: ManagementConfig,
    pub risk: RiskConfig,
    pub schedule: ScheduleConfig,
    pub llm: LlmConfig,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub strategy: StrategyConfig,
    #[serde(default)]
    pub dual_strategy: DualStrategyConfig,
    #[serde(default)]
    pub tokens: TokensConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub jupiter: JupiterConfig,
    #[serde(default)]
    pub gmgn: GmgnConfig,
    #[serde(default)]
    pub hive_mind: HiveMindConfig,
    // Accept both "indicators" and the dashboard/flat-config "chartIndicators".
    #[serde(default, alias = "chartIndicators")]
    pub indicators: IndicatorsConfig,
    #[serde(default)]
    pub darwin: DarwinConfig,
    #[serde(default)]
    pub quickflip: QuickFlipConfig,
}

/// Quick-flip mode: deterministic volume-spike scalping (separate from the LLM
/// agent). All fields default so existing configs keep working; `enabled` is
/// off by default and toggled at runtime via the web control API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickFlipConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_qf_poll_secs")]
    pub poll_secs: u64,
    #[serde(default = "default_qf_min_vol_per_min")]
    pub min_vol_per_min: f64,
    #[serde(default = "default_qf_min_fee_tvl")]
    pub min_fee_tvl_ratio: f64,
    #[serde(default = "default_qf_deploy_amount")]
    pub deploy_amount_sol: f64,
    #[serde(default = "default_qf_bins_below")]
    pub bins_below: i64,
    #[serde(default)]
    pub bins_above: i64,
    #[serde(default = "default_qf_max_hold_min")]
    pub max_hold_min: u32,
    #[serde(default = "default_qf_vol_fade_ratio")]
    pub vol_fade_ratio: f64,
    #[serde(default = "default_qf_take_profit_usd")]
    pub take_profit_usd: f64,
    #[serde(default = "default_qf_min_tvl")]
    pub min_tvl: f64,
    #[serde(default = "default_qf_max_tvl")]
    pub max_tvl: f64,
}

fn default_qf_poll_secs() -> u64 {
    45
}
fn default_qf_min_vol_per_min() -> f64 {
    30_000.0
}
fn default_qf_min_fee_tvl() -> f64 {
    1.0
}
fn default_qf_deploy_amount() -> f64 {
    0.2
}
fn default_qf_bins_below() -> i64 {
    34
}
fn default_qf_max_hold_min() -> u32 {
    10
}
fn default_qf_vol_fade_ratio() -> f64 {
    0.5
}
fn default_qf_take_profit_usd() -> f64 {
    0.5
}
fn default_qf_min_tvl() -> f64 {
    5_000.0
}
fn default_qf_max_tvl() -> f64 {
    300_000.0
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_secs: default_qf_poll_secs(),
            min_vol_per_min: default_qf_min_vol_per_min(),
            min_fee_tvl_ratio: default_qf_min_fee_tvl(),
            deploy_amount_sol: default_qf_deploy_amount(),
            bins_below: default_qf_bins_below(),
            bins_above: 0,
            max_hold_min: default_qf_max_hold_min(),
            vol_fade_ratio: default_qf_vol_fade_ratio(),
            take_profit_usd: default_qf_take_profit_usd(),
            min_tvl: default_qf_min_tvl(),
            max_tvl: default_qf_max_tvl(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreeningConfig {
    pub min_fee_active_tvl_ratio: f64,
    pub min_tvl: f64,
    #[serde(default)]
    pub max_tvl: Option<f64>,
    pub min_volume: f64,
    pub min_organic: f64,
    #[serde(default = "default_min_quote_organic")]
    pub min_quote_organic: f64,
    pub min_holders: u64,
    pub min_mcap: f64,
    pub max_mcap: f64,
    pub min_bin_step: u16,
    pub max_bin_step: u16,
    #[serde(default = "default_timeframe")]
    pub timeframe: String,
    #[serde(default = "default_category")]
    pub category: String,
    pub min_token_fees_sol: f64,
    #[serde(default = "default_max_bot_holders")]
    pub max_bot_holders_pct: f64,
    #[serde(default)]
    pub max_bundlers_pct: Option<f64>,
    #[serde(default = "default_max_top10")]
    pub max_top10_pct: f64,
    #[serde(default)]
    pub blocked_launchpads: Vec<String>,
    #[serde(default)]
    pub allowed_launchpads: Vec<String>,
    #[serde(default)]
    pub exclude_high_supply_concentration: bool,
    #[serde(default)]
    pub min_token_age_hours: Option<f64>,
    #[serde(default)]
    pub max_token_age_hours: Option<f64>,
    #[serde(default)]
    pub use_discord_signals: bool,
    #[serde(default)]
    pub discord_signal_mode: Option<String>,
    #[serde(default = "default_avoid_pvp_symbols")]
    pub avoid_pvp_symbols: bool,
    #[serde(default)]
    pub block_pvp_symbols: bool,
}

fn default_min_quote_organic() -> f64 {
    0.0
}
fn default_timeframe() -> String {
    "1h".to_string()
}
fn default_category() -> String {
    "trending".to_string()
}
fn default_max_bot_holders() -> f64 {
    30.0
}
fn default_max_top10() -> f64 {
    60.0
}
fn default_avoid_pvp_symbols() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementConfig {
    pub deploy_amount_sol: f64,
    pub gas_reserve: f64,
    pub position_size_pct: f64,
    pub min_sol_to_open: f64,
    pub out_of_range_wait_minutes: u32,
    #[serde(default = "default_oor_cooldown_trigger_count")]
    pub oor_cooldown_trigger_count: u32,
    #[serde(default = "default_oor_cooldown_hours")]
    pub oor_cooldown_hours: u32,
    #[serde(default = "default_repeat_deploy_cooldown_enabled")]
    pub repeat_deploy_cooldown_enabled: bool,
    #[serde(default = "default_repeat_deploy_cooldown_trigger_count")]
    pub repeat_deploy_cooldown_trigger_count: u32,
    #[serde(default = "default_repeat_deploy_cooldown_hours")]
    pub repeat_deploy_cooldown_hours: u32,
    #[serde(default = "default_repeat_deploy_cooldown_scope")]
    pub repeat_deploy_cooldown_scope: String,
    #[serde(default)]
    pub repeat_deploy_cooldown_min_fee_earned_pct: f64,
    pub take_profit_pct: Option<f64>,
    #[serde(default = "default_management_interval")]
    pub management_interval_min: u32,
    #[serde(default = "default_screening_interval")]
    pub screening_interval_min: u32,
    // ── Trailing TP ──────────────────────────────────────────
    #[serde(default)]
    pub trailing_take_profit: bool,
    #[serde(default = "default_trailing_trigger_pct")]
    pub trailing_trigger_pct: f64,
    #[serde(default = "default_trailing_drop_pct")]
    pub trailing_drop_pct: f64,
    // ── Claim / Yield thresholds ─────────────────────────────
    #[serde(default = "default_min_claim_amount")]
    pub min_claim_amount: f64,
    #[serde(default = "default_min_fee_per_tvl_24h")]
    pub min_fee_per_tvl_24h: f64,
    #[serde(default = "default_min_age_before_yield_check")]
    pub min_age_before_yield_check: u32,
    // ── Pump / OOR bins ──────────────────────────────────────
    #[serde(default = "default_out_of_range_bins_to_close")]
    pub out_of_range_bins_to_close: i32,
    // ── Over-extension take-profit exit (Evil Panda) ─────────
    // Bank the bounce when the base token is over-extended to the upside:
    // RSI(2) >= threshold OR Bollinger %B >= upper. Only fires when the
    // position is in profit (>= exit_min_profit_pct) so it never realizes a loss.
    #[serde(default = "default_exit_overextended_enabled")]
    pub exit_overextended_enabled: bool,
    #[serde(default = "default_exit_rsi_threshold")]
    pub exit_rsi_threshold: f64,
    #[serde(default = "default_exit_bb_upper_pctb")]
    pub exit_bb_upper_pctb: f64,
    #[serde(default)]
    pub exit_min_profit_pct: f64,
    // ── Token-2022 gate ──────────────────────────────────────
    // Refuse to deploy into pools whose base token is Token-2022: the wp SDK
    // derives the user token account under classic SPL, so claim/close always
    // fails with AccountNotInitialized on user_token_y and the position gets
    // stuck. Set false once the commons migration handles Token-2022.
    #[serde(default = "default_skip_token_2022")]
    pub skip_token_2022: bool,
    // ── Display mode ─────────────────────────────────────────
    #[serde(default)]
    pub sol_mode: bool,
}

fn default_skip_token_2022() -> bool {
    true
}
fn default_exit_overextended_enabled() -> bool {
    true
}
fn default_exit_rsi_threshold() -> f64 {
    90.0
}
fn default_exit_bb_upper_pctb() -> f64 {
    1.0
}

fn default_management_interval() -> u32 {
    10
}
fn default_screening_interval() -> u32 {
    30
}
fn default_oor_cooldown_trigger_count() -> u32 {
    3
}
fn default_oor_cooldown_hours() -> u32 {
    12
}
fn default_repeat_deploy_cooldown_enabled() -> bool {
    true
}
fn default_repeat_deploy_cooldown_trigger_count() -> u32 {
    3
}
fn default_repeat_deploy_cooldown_hours() -> u32 {
    12
}
fn default_repeat_deploy_cooldown_scope() -> String {
    "token".to_string()
}
fn default_trailing_trigger_pct() -> f64 {
    5.0
}
fn default_trailing_drop_pct() -> f64 {
    3.0
}
fn default_min_claim_amount() -> f64 {
    0.01
}
fn default_min_fee_per_tvl_24h() -> f64 {
    0.0005
}
fn default_min_age_before_yield_check() -> u32 {
    60
}
fn default_out_of_range_bins_to_close() -> i32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskConfig {
    pub max_deploy_amount: f64,
    pub max_positions: u32,
    #[serde(default)]
    pub stop_loss_pct: Option<f64>,
    #[serde(default = "default_cooldown_loss")]
    pub cooldown_loss_pct: f64,
    #[serde(default = "default_cooldown_duration")]
    pub cooldown_duration_min: u32,
    /// Safety exit: once a position's max drawdown hits the trigger, lower its
    /// take-profit to the (smaller) safety level so a minor rebound banks it.
    #[serde(default = "default_safety_exit_enabled")]
    pub safety_exit_enabled: bool,
    #[serde(default = "default_safety_exit_trigger")]
    pub safety_exit_trigger_pct: f64,
    #[serde(default = "default_safety_exit_tp")]
    pub safety_exit_tp_pct: f64,
    /// Suppress all exits for positions younger than this (initial-noise guard).
    #[serde(default = "default_min_position_duration")]
    pub min_position_duration_min: u32,
}

fn default_cooldown_loss() -> f64 {
    -5.0
}
fn default_cooldown_duration() -> u32 {
    60
}
fn default_safety_exit_enabled() -> bool {
    true
}
fn default_safety_exit_trigger() -> f64 {
    -8.0
}
fn default_safety_exit_tp() -> f64 {
    0.0
}
fn default_min_position_duration() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleConfig {
    pub management_interval_min: u32,
    pub screening_interval_min: u32,
    #[serde(default = "default_pnl_poll_interval")]
    pub pnl_poll_interval_secs: u32,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_min: u32,
}

fn default_pnl_poll_interval() -> u32 {
    30
}
fn default_sync_interval() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    pub management_model: String,
    pub screening_model: String,
    pub general_model: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
}

fn default_base_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}
fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> u32 {
    4096
}
fn default_max_steps() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyConfig {
    pub min_bins_below: u32,
    pub max_bins_below: u32,
    #[serde(default = "default_min_safe_bins")]
    pub min_safe_bins_below: u32,
    /// Target fraction of downside price the single-side range should cover
    /// (e.g. 0.40 = 40%). When the caller doesn't pin bins_below explicitly,
    /// the deploy sizes the bin count from this and the pool's bin_step so the
    /// range stays consistent across pools instead of going out-of-range fast
    /// on tight (low-bin_step) pools.
    #[serde(default = "default_target_downside_pct")]
    pub target_downside_pct: f64,
}

fn default_min_safe_bins() -> u32 {
    35
}

fn default_target_downside_pct() -> f64 {
    0.40
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            min_bins_below: 15,
            max_bins_below: 50,
            min_safe_bins_below: 35,
            target_downside_pct: default_target_downside_pct(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DualStrategyConfig {
    pub enabled: bool,
    pub primary_pct: f64,
    pub safeguard_oor_wait_min: u32,
    pub aggressive_oor_wait_min: u32,
}

impl Default for DualStrategyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            primary_pct: 0.6,
            safeguard_oor_wait_min: 60,
            aggressive_oor_wait_min: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokensConfig {
    #[serde(default = "default_sol_mint")]
    pub sol_mint: String,
    #[serde(default = "default_usdc_mint")]
    pub usdc_mint: String,
}

fn default_sol_mint() -> String {
    "So11111111111111111111111111111111111111112".to_string()
}
fn default_usdc_mint() -> String {
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    #[serde(default)]
    pub helius_api_key: Option<String>,
    #[serde(default)]
    pub helius_rpc_url: Option<String>,
    /// Spare RPC endpoints, tried in order when the primary stops answering.
    #[serde(default)]
    pub rpc_fallback_urls: Vec<String>,
    #[serde(default)]
    pub agent_meridian_base: Option<String>,
    #[serde(default)]
    pub agent_meridian_key: Option<String>,
    #[serde(default)]
    pub lp_agent_relay_enabled: bool,
    #[serde(default)]
    pub telegram_bot_token: Option<String>,
    #[serde(default)]
    pub telegram_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JupiterConfig {
    #[serde(default)]
    pub referral_account: Option<String>,
    #[serde(default = "default_referral_fee_bps")]
    pub referral_fee_bps: u32,
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_referral_fee_bps() -> u32 {
    50
}

/// GMGN OpenAPI fee source. Used as the primary `minTokenFeesSol` gate signal,
/// falling back to pool/Jupiter fees when no API key is configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmgnConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_gmgn_base_url")]
    pub base_url: String,
    #[serde(default = "default_gmgn_request_delay_ms")]
    pub request_delay_ms: u64,
    #[serde(default = "default_gmgn_max_retries")]
    pub max_retries: u32,
    /// Reject a token whose top-10 holders control more than this fraction
    /// (0.0–1.0) of supply — concentrated holders are the strongest rug signal.
    /// Set to 0 or >= 1.0 to disable the gate. Default 0.8 (only egregious).
    #[serde(default = "default_gmgn_max_top10")]
    pub max_top10_holder_rate: f64,
}

impl Default for GmgnConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: default_gmgn_base_url(),
            request_delay_ms: default_gmgn_request_delay_ms(),
            max_retries: default_gmgn_max_retries(),
            max_top10_holder_rate: default_gmgn_max_top10(),
        }
    }
}

fn default_gmgn_max_top10() -> f64 {
    0.8
}

fn default_gmgn_base_url() -> String {
    "https://openapi.gmgn.ai".to_string()
}

fn default_gmgn_request_delay_ms() -> u64 {
    2500
}

fn default_gmgn_max_retries() -> u32 {
    2
}

/// HiveMind shared-learning sync (Agent Meridian). Pulls shared lessons/presets
/// and pushes closed-position performance events. Enabled only when both `url`
/// and `api_key` are set; private keys and wallet balances are never sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiveMindConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default = "default_hive_pull_mode")]
    pub pull_mode: String,
}

impl Default for HiveMindConfig {
    fn default() -> Self {
        Self {
            url: None,
            api_key: None,
            agent_id: None,
            pull_mode: default_hive_pull_mode(),
        }
    }
}

fn default_hive_pull_mode() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub presets: Vec<String>,
    #[serde(default)]
    pub entry_preset: Option<String>,
    #[serde(default)]
    pub exit_preset: Option<String>,
    #[serde(default = "default_indicator_rsi_length")]
    pub rsi_length: u32,
    #[serde(default = "default_indicator_intervals")]
    pub intervals: Vec<String>,
    #[serde(default = "default_indicator_candles")]
    pub candles: u32,
    #[serde(default = "default_indicator_rsi_oversold")]
    pub rsi_oversold: f64,
    #[serde(default = "default_indicator_rsi_overbought")]
    pub rsi_overbought: f64,
    #[serde(default)]
    pub require_all_intervals: bool,
    /// Bollinger %B entry gate: only deploy when the latest 5m %B is >= this
    /// (price over-extended up → mean-reversion pullback into our below-range LP
    /// is more likely). Active only when `enabled` is true.
    #[serde(default = "default_indicator_bb_percent_b_min")]
    pub bb_percent_b_min: f64,
}

impl Default for IndicatorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            presets: vec![],
            entry_preset: None,
            exit_preset: None,
            rsi_length: default_indicator_rsi_length(),
            intervals: default_indicator_intervals(),
            candles: default_indicator_candles(),
            rsi_oversold: default_indicator_rsi_oversold(),
            rsi_overbought: default_indicator_rsi_overbought(),
            require_all_intervals: false,
            bb_percent_b_min: default_indicator_bb_percent_b_min(),
        }
    }
}

fn default_indicator_rsi_length() -> u32 {
    2
}
fn default_indicator_intervals() -> Vec<String> {
    vec!["5_MINUTE".to_string()]
}
fn default_indicator_candles() -> u32 {
    298
}
fn default_indicator_rsi_oversold() -> f64 {
    30.0
}
fn default_indicator_rsi_overbought() -> f64 {
    80.0
}
fn default_indicator_bb_percent_b_min() -> f64 {
    0.8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DarwinConfig {
    #[serde(default = "default_darwin_enabled")]
    pub enabled: bool,
    #[serde(default = "default_darwin_window_days")]
    pub window_days: u64,
    #[serde(default = "default_darwin_recalc_every")]
    pub recalc_every: u64,
    #[serde(default = "default_darwin_boost_factor")]
    pub boost_factor: f64,
    #[serde(default = "default_darwin_decay_factor")]
    pub decay_factor: f64,
    #[serde(default = "default_darwin_weight_floor")]
    pub weight_floor: f64,
    #[serde(default = "default_darwin_weight_ceiling")]
    pub weight_ceiling: f64,
    #[serde(default = "default_darwin_min_samples")]
    pub min_samples: u64,
}

impl Default for DarwinConfig {
    fn default() -> Self {
        Self {
            enabled: default_darwin_enabled(),
            window_days: default_darwin_window_days(),
            recalc_every: default_darwin_recalc_every(),
            boost_factor: default_darwin_boost_factor(),
            decay_factor: default_darwin_decay_factor(),
            weight_floor: default_darwin_weight_floor(),
            weight_ceiling: default_darwin_weight_ceiling(),
            min_samples: default_darwin_min_samples(),
        }
    }
}

fn default_darwin_enabled() -> bool {
    true
}
fn default_darwin_window_days() -> u64 {
    60
}
fn default_darwin_recalc_every() -> u64 {
    5
}
fn default_darwin_boost_factor() -> f64 {
    1.05
}
fn default_darwin_decay_factor() -> f64 {
    0.95
}
fn default_darwin_weight_floor() -> f64 {
    0.3
}
fn default_darwin_weight_ceiling() -> f64 {
    2.5
}
fn default_darwin_min_samples() -> u64 {
    10
}

impl Default for Config {
    fn default() -> Self {
        let screening_defaults = super::screening_scales::screening_defaults_for_timeframe("4h");
        Self {
            screening: ScreeningConfig {
                min_fee_active_tvl_ratio: screening_defaults.min_fee_active_tvl_ratio,
                min_tvl: 10_000.0,
                max_tvl: None,
                min_volume: screening_defaults.min_volume,
                min_organic: 60.0,
                min_quote_organic: 0.0,
                min_holders: 500,
                min_mcap: 150_000.0,
                max_mcap: 10_000_000.0,
                min_bin_step: 80,
                max_bin_step: 125,
                timeframe: screening_defaults.timeframe,
                category: "trending".to_string(),
                min_token_fees_sol: 30.0,
                max_bot_holders_pct: 30.0,
                max_bundlers_pct: None,
                max_top10_pct: 60.0,
                blocked_launchpads: vec![],
                allowed_launchpads: vec![],
                exclude_high_supply_concentration: false,
                min_token_age_hours: None,
                max_token_age_hours: None,
                use_discord_signals: false,
                discord_signal_mode: None,
                avoid_pvp_symbols: true,
                block_pvp_symbols: false,
            },
            management: ManagementConfig {
                deploy_amount_sol: 0.5,
                gas_reserve: 0.2,
                position_size_pct: 0.35,
                min_sol_to_open: 0.55,
                out_of_range_wait_minutes: 30,
                oor_cooldown_trigger_count: 3,
                oor_cooldown_hours: 12,
                repeat_deploy_cooldown_enabled: true,
                repeat_deploy_cooldown_trigger_count: 3,
                repeat_deploy_cooldown_hours: 12,
                repeat_deploy_cooldown_scope: "token".to_string(),
                repeat_deploy_cooldown_min_fee_earned_pct: 0.0,
                take_profit_pct: None,
                management_interval_min: 10,
                screening_interval_min: 30,
                trailing_take_profit: false,
                trailing_trigger_pct: 5.0,
                trailing_drop_pct: 3.0,
                min_claim_amount: 0.01,
                min_fee_per_tvl_24h: 0.0005,
                min_age_before_yield_check: 60,
                out_of_range_bins_to_close: 50,
                exit_overextended_enabled: default_exit_overextended_enabled(),
                exit_rsi_threshold: default_exit_rsi_threshold(),
                exit_bb_upper_pctb: default_exit_bb_upper_pctb(),
                exit_min_profit_pct: 0.0,
                skip_token_2022: default_skip_token_2022(),
                sol_mode: false,
            },
            risk: RiskConfig {
                max_deploy_amount: 50.0,
                max_positions: 3,
                stop_loss_pct: None,
                cooldown_loss_pct: -5.0,
                cooldown_duration_min: 60,
                safety_exit_enabled: default_safety_exit_enabled(),
                safety_exit_trigger_pct: default_safety_exit_trigger(),
                safety_exit_tp_pct: default_safety_exit_tp(),
                min_position_duration_min: default_min_position_duration(),
            },
            schedule: ScheduleConfig {
                management_interval_min: 10,
                screening_interval_min: 30,
                pnl_poll_interval_secs: 30,
                sync_interval_min: 5,
            },
            llm: LlmConfig {
                management_model: "openrouter/healer-alpha".to_string(),
                screening_model: "openrouter/healer-alpha".to_string(),
                general_model: "openrouter/healer-alpha".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: None,
                temperature: 0.7,
                max_tokens: 4096,
                max_steps: 20,
            },
            dry_run: false,
            strategy: StrategyConfig {
                min_bins_below: 15,
                max_bins_below: 50,
                min_safe_bins_below: 35,
                target_downside_pct: default_target_downside_pct(),
            },
            dual_strategy: DualStrategyConfig::default(),
            tokens: TokensConfig::default(),
            api: ApiConfig::default(),
            jupiter: JupiterConfig::default(),
            gmgn: GmgnConfig::default(),
            hive_mind: HiveMindConfig::default(),
            indicators: IndicatorsConfig::default(),
            darwin: DarwinConfig::default(),
            quickflip: QuickFlipConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_example_config_without_explicit_strategy() {
        let raw = include_str!("../../user-config.example.json");
        let config: Config = serde_json::from_str(raw).expect("example config should parse");

        assert_eq!(config.strategy.min_bins_below, 15);
        assert_eq!(config.strategy.max_bins_below, 50);
        assert_eq!(config.strategy.min_safe_bins_below, 35);
    }

    #[test]
    fn default_config_enables_darwin_signal_weighting() {
        let config = Config::default();

        assert!(config.darwin.enabled);
        assert_eq!(config.darwin.window_days, 60);
        assert_eq!(config.darwin.recalc_every, 5);
        assert_eq!(config.darwin.boost_factor, 1.05);
        assert_eq!(config.darwin.decay_factor, 0.95);
        assert_eq!(config.darwin.weight_floor, 0.3);
        assert_eq!(config.darwin.weight_ceiling, 2.5);
        assert_eq!(config.darwin.min_samples, 10);
    }
}
