use crate::config::types::ScreeningConfig;
use crate::models::pool::{CondensedPool, PoolDiscoveryResponse, PoolToken, RawPool};
use crate::tools::discord_signals::{merge_discord_signal_pools, DiscordSignalStore};
use crate::utils::logger::module::info;
use anyhow::{Context, Result};
use reqwest::Client;

const POOL_DISCOVERY_BASE: &str = "https://pool-discovery-api.datapi.meteora.ag";
const MIN_VOLATILITY_TIMEFRAME: &str = "30m";
const PVP_MIN_ACTIVE_TVL: f64 = 5_000.0;
const PVP_MIN_HOLDERS: u64 = 500;
/// Score added per GMGN smart-money holder (capped at 25) on a candidate's base
/// token. Modest vs the base score so it nudges ranking without dominating it.
const SMART_MONEY_SCORE_WEIGHT: f64 = 25.0;
const PVP_MIN_GLOBAL_FEES_SOL: f64 = 30.0;

static TIMEFRAME_MINUTES: &[(&str, u32)] = &[
    ("5m", 5),
    ("30m", 30),
    ("1h", 60),
    ("2h", 120),
    ("4h", 240),
    ("12h", 720),
    ("24h", 1440),
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct FilteredPoolExample {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScreeningResult {
    pub total_screened: usize,
    pub candidates: Vec<CondensedPool>,
    pub filtered_examples: Vec<FilteredPoolExample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_candidate_skip_reason: Option<String>,
}

pub struct Screener {
    client: Client,
}

/// Entry-time market metrics captured at deploy/adopt so the Darwin learner
/// (`signal_weights`) and lessons have real screening signals to attribute
/// outcomes to. The LLM's `deploy_position` args only carry pool/amount/bins,
/// so these are fetched from the pool's screening data instead.
#[derive(Debug, Clone, Default)]
pub struct EntryMetrics {
    pub mcap: Option<f64>,
    pub tvl: Option<f64>,
    pub volume: Option<f64>,
    pub holders: Option<u64>,
    pub volatility: Option<f64>,
    pub fee_tvl_ratio: Option<f64>,
    pub organic_score: Option<f64>,
    pub bin_step: Option<u16>,
}

/// Fetch entry metrics for a pool by re-using the screening pool-detail API.
/// Returns `None` on any API/parse failure (callers should degrade gracefully).
pub async fn fetch_entry_metrics(pool_address: &str, timeframe: &str) -> Option<EntryMetrics> {
    let detail = Screener::new()
        .get_pool_detail(pool_address, timeframe)
        .await
        .ok()
        .flatten()?;
    Some(EntryMetrics {
        mcap: detail.base_token_mcap,
        tvl: detail.tvl.or(detail.active_tvl),
        volume: detail.volume,
        holders: detail.base_token_holders,
        volatility: detail.volatility,
        fee_tvl_ratio: detail.fee_active_tvl_ratio,
        organic_score: detail.base_token_organic_score,
        bin_step: detail.dlmm_bin_step,
    })
}

impl Screener {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Fetch pools from Meteora Pool Discovery API with filters
    pub async fn discover_pools(
        &self,
        s: &ScreeningConfig,
        page_size: u32,
    ) -> Result<Vec<RawPool>> {
        let now_ms = chrono::Utc::now().timestamp_millis() as f64;

        let mut filters = vec![
            "base_token_has_critical_warnings=false".to_string(),
            "quote_token_has_critical_warnings=false".to_string(),
            "base_token_has_high_single_ownership=false".to_string(),
            "pool_type=dlmm".to_string(),
            format!("base_token_market_cap>={}", s.min_mcap),
            format!("base_token_market_cap<={}", s.max_mcap),
            format!("base_token_holders>={}", s.min_holders),
            format!("volume>={}", s.min_volume),
            format!("tvl>={}", s.min_tvl),
            format!("dlmm_bin_step>={}", s.min_bin_step),
            format!("dlmm_bin_step<={}", s.max_bin_step),
            format!("fee_active_tvl_ratio>={}", s.min_fee_active_tvl_ratio),
            format!("base_token_organic_score>={}", s.min_organic),
            format!("quote_token_organic_score>={}", s.min_quote_organic),
        ];

        if s.exclude_high_supply_concentration {
            filters.push("base_token_has_high_supply_concentration=false".to_string());
        }
        if let Some(max_tvl) = s.max_tvl {
            filters.push(format!("tvl<={}", max_tvl));
        }
        if let Some(hours) = s.min_token_age_hours {
            filters.push(format!(
                "base_token_created_at<={}",
                now_ms - hours * 3_600_000.0
            ));
        }
        if let Some(hours) = s.max_token_age_hours {
            filters.push(format!(
                "base_token_created_at>={}",
                now_ms - hours * 3_600_000.0
            ));
        }
        if !s.allowed_launchpads.is_empty() {
            filters.push(format!(
                "base_token_launchpad=[{}]",
                s.allowed_launchpads.join(",")
            ));
        }

        let filter_str = filters.join("&&");
        let url = format!(
            "{}/pools?page_size={}&filter_by={}&timeframe={}&category={}",
            POOL_DISCOVERY_BASE,
            page_size,
            urlencoding::encode(&filter_str),
            urlencoding::encode(&s.timeframe),
            urlencoding::encode(&s.category),
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Pool Discovery API request failed")?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Pool Discovery API error: {}",
                resp.status()
            ));
        }

        let data: PoolDiscoveryResponse = resp.json().await?;
        let mut raw_pools = data.data.unwrap_or_default();
        if s.use_discord_signals {
            match DiscordSignalStore::load_default() {
                Ok(store) => {
                    let pending = store.pending_cloned();
                    if !pending.is_empty() {
                        info(
                            "screening",
                            &format!(
                                "Merging {} pending Discord signal(s) from {}",
                                pending.len(),
                                store.path.display()
                            ),
                        );
                    }
                    raw_pools = merge_discord_signal_pools(
                        raw_pools,
                        &pending,
                        s.discord_signal_mode.as_deref(),
                    );
                }
                Err(error) => info(
                    "screening",
                    &format!("Discord signal queue load failed: {error}"),
                ),
            }
        }
        Ok(raw_pools)
    }

    /// Get pool detail for a specific address
    pub async fn get_pool_detail(
        &self,
        pool_address: &str,
        timeframe: &str,
    ) -> Result<Option<RawPool>> {
        let url = format!(
            "{}/pools?page_size=1&filter_by={}&timeframe={}",
            POOL_DISCOVERY_BASE,
            urlencoding::encode(&format!("pool_address={}", pool_address)),
            urlencoding::encode(timeframe),
        );

        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Pool detail API error: {}", resp.status()));
        }

        let data: PoolDiscoveryResponse = resp.json().await?;
        Ok(data.data.and_then(|v| v.into_iter().next()))
    }

    /// Apply volatility from a longer timeframe if needed
    pub async fn apply_volatility_timeframe(
        &self,
        pools: &mut [RawPool],
        source_tf: &str,
    ) -> Result<()> {
        let vol_tf = get_volatility_timeframe(source_tf);
        if vol_tf == source_tf {
            return Ok(());
        }

        let pool_addrs: Vec<String> = pools
            .iter()
            .filter_map(|p| p.pool_address.clone())
            .collect();

        for addr in pool_addrs {
            if let Ok(Some(detail)) = self.get_pool_detail(&addr, vol_tf).await {
                if let Some(pool) = pools
                    .iter_mut()
                    .find(|p| p.pool_address.as_deref() == Some(&addr))
                {
                    if let Some(v) = detail.volatility {
                        pool.volatility = Some(v);
                    }
                    if let Some(v) = detail.volume {
                        pool.volume = Some(v);
                    }
                }
            }
        }
        Ok(())
    }

    /// Full screening pipeline: discover → filter → condense → score → sort
    pub async fn get_top_candidates(
        &self,
        s: &ScreeningConfig,
        limit: usize,
    ) -> Result<Vec<CondensedPool>> {
        Ok(self
            .get_top_candidates_with_rejections(s, limit)
            .await?
            .candidates)
    }

    pub async fn get_top_candidates_with_rejections(
        &self,
        s: &ScreeningConfig,
        limit: usize,
    ) -> Result<ScreeningResult> {
        let mut raw = self.discover_pools(s, 50).await?;
        info(
            "screening",
            &format!("Discovery returned {} raw pools", raw.len()),
        );
        self.apply_volatility_timeframe(&mut raw, &s.timeframe)
            .await?;

        let result = screen_raw_pools(raw, s, limit);

        info(
            "screening",
            &format!("{} pools passed filters", result.candidates.len()),
        );

        Ok(result)
    }

    /// Override each candidate's `fees_sol` with GMGN's cumulative token fee
    /// when a GMGN API key is configured. This is the primary `minTokenFeesSol`
    /// signal; pools keep their pool/Jupiter fee figure on miss (faithful to the
    /// original JS fallback behavior). No-op when GMGN is not configured.
    pub async fn enrich_candidate_fees(
        &self,
        candidates: &mut [CondensedPool],
        config: &crate::config::Config,
    ) {
        if !crate::tools::gmgn::has_gmgn_api_key(config) {
            return;
        }
        for candidate in candidates.iter_mut() {
            if candidate.base.mint.is_empty() {
                continue;
            }
            if let Some(fees) =
                crate::tools::gmgn::get_gmgn_token_fees(&candidate.base.mint, config).await
            {
                if let Some(total) = fees.total_fee {
                    candidate.fees_sol = total;
                }
            }
            // Smart-money quality signal: count GMGN-tagged smart-money holders
            // on the base token and nudge the score up. Soft preference — pools
            // with smart backing rank higher — never a hard gate.
            if let Some(smart) =
                crate::tools::gmgn::get_smart_money_count(&candidate.base.mint, config).await
            {
                candidate.smart_money_count = Some(smart);
                candidate.score += (smart.min(25) as f64) * SMART_MONEY_SCORE_WEIGHT;
            }
        }
        // Re-rank after the smart-money boost so preferred pools surface first.
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn condense_raw_pool(pool: &RawPool) -> CondensedPool {
    let base = pool.token_x.as_ref();
    let base_mint = base.and_then(|b| b.address.clone()).unwrap_or_default();
    let base_symbol = base
        .and_then(|b| b.symbol.clone())
        .unwrap_or_else(|| "?".to_string());
    let base_organic = base
        .and_then(|b| b.organic_score)
        .or(pool.base_token_organic_score)
        .unwrap_or(0.0);
    let quote_organic = pool
        .token_y
        .as_ref()
        .and_then(|q| q.organic_score)
        .or(pool.quote_token_organic_score)
        .unwrap_or(0.0);
    let mcap = base
        .and_then(|b| b.market_cap)
        .or(pool.base_token_mcap)
        .unwrap_or(0.0);

    CondensedPool {
        name: pool
            .name
            .clone()
            .unwrap_or_else(|| pool.pool_address.clone().unwrap_or_default()),
        pool_address: pool.pool_address.clone().unwrap_or_default(),
        base: PoolToken {
            mint: base_mint,
            symbol: base_symbol,
            organic: base_organic,
            mcap: Some(mcap),
            icon: base.and_then(|b| b.icon.clone()),
        },
        quote_organic,
        tvl: pool.tvl.or(pool.active_tvl).unwrap_or(0.0),
        volume: pool.volume.unwrap_or(0.0),
        fee_active_tvl_ratio: pool.fee_active_tvl_ratio.unwrap_or(0.0),
        volatility: pool.volatility.unwrap_or(0.0),
        bin_step: pool.dlmm_bin_step.unwrap_or(100),
        score: score_candidate(pool),
        holders: pool.base_token_holders.unwrap_or(0),
        mcap,
        fees_sol: pool.fee.unwrap_or(0.0),
        launchpad: pool_launchpad(pool),
        dev: None,
        bundlers_pct: None,
        top10_pct: None,
        discord_signal: pool
            .extra
            .get("discord_signal")
            .and_then(|value| value.as_bool())
            .filter(|value| *value),
        is_pvp: None,
        pvp_risk: None,
        pvp_symbol: None,
        pvp_rival_name: None,
        pvp_rival_mint: None,
        pvp_rival_pool: None,
        pvp_rival_tvl: None,
        pvp_rival_holders: None,
        pvp_rival_fees: None,
        smart_money_count: None,
    }
}

pub fn screen_raw_pools(raw: Vec<RawPool>, s: &ScreeningConfig, limit: usize) -> ScreeningResult {
    let total_screened = raw.len();
    let mut filtered_examples = Vec::new();
    let mut candidates = Vec::new();

    for pool in raw {
        if let Some(reason) = raw_pool_screening_reject_reason(&pool, s) {
            filtered_examples.push(FilteredPoolExample {
                name: pool
                    .name
                    .clone()
                    .or_else(|| pool.pool_address.clone())
                    .unwrap_or_else(|| "unknown pool".to_string()),
                reason,
            });
        } else {
            candidates.push(condense_raw_pool(&pool));
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(limit);
    apply_pvp_risk_policy_with_rejections(&mut candidates, s, &mut filtered_examples);

    let single_candidate_skip_reason = if candidates.len() == 1 {
        single_candidate_skip_reason(&candidates[0])
    } else {
        None
    };

    ScreeningResult {
        total_screened,
        candidates,
        filtered_examples,
        single_candidate_skip_reason,
    }
}

pub fn single_candidate_skip_reason(candidate: &CondensedPool) -> Option<String> {
    if candidate.is_pvp == Some(true) {
        return Some(format!(
            "only one candidate survived, but it has high PVP risk against {}",
            candidate
                .pvp_rival_name
                .as_deref()
                .unwrap_or("a rival pool")
        ));
    }

    if candidate.fees_sol < PVP_MIN_GLOBAL_FEES_SOL && candidate.discord_signal != Some(true) {
        return Some(format!(
            "only one candidate survived and token fees are weak ({:.2} SOL); original JS requires extra conviction before lone-candidate deploys",
            candidate.fees_sol
        ));
    }

    if candidate.base.organic < 55.0 && candidate.discord_signal != Some(true) {
        return Some(format!(
            "only one candidate survived and organic score is marginal ({:.1}); require stronger narrative/smart-wallet confirmation",
            candidate.base.organic
        ));
    }

    None
}

pub fn raw_pool_screening_reject_reason(pool: &RawPool, s: &ScreeningConfig) -> Option<String> {
    let base = pool.token_x.as_ref();
    let quote = pool.token_y.as_ref();
    let mcap = base.and_then(|b| b.market_cap).or(pool.base_token_mcap);
    let holders = pool.base_token_holders;
    let tvl = pool.tvl.or(pool.active_tvl);
    let volatility = pool.volatility;
    let bin_step = pool.dlmm_bin_step;
    let fee_ratio = pool.fee_active_tvl_ratio;
    let base_organic = base
        .and_then(|b| b.organic_score)
        .or(pool.base_token_organic_score);
    let quote_organic = quote
        .and_then(|q| q.organic_score)
        .or(pool.quote_token_organic_score);
    let launchpad = pool_launchpad(pool);
    let created_at = base.and_then(|b| b.created_at);

    if s.exclude_high_supply_concentration
        && pool.base_token_has_high_supply_concentration == Some(true)
    {
        return Some("base token has high supply concentration".into());
    }
    if pool.base_token_has_critical_warnings == Some(true) {
        return Some("base token has critical warnings".into());
    }
    if pool.quote_token_has_critical_warnings == Some(true) {
        return Some("quote token has critical warnings".into());
    }
    if pool.base_token_has_high_single_ownership == Some(true) {
        return Some("base token has high single ownership".into());
    }
    if pool
        .pool_type
        .as_deref()
        .is_some_and(|pool_type| pool_type != "dlmm")
    {
        return Some(format!(
            "pool_type {} is not dlmm",
            pool.pool_type.as_deref().unwrap_or("unknown")
        ));
    }

    if mcap.is_none_or(|mcap| mcap < s.min_mcap) {
        return Some(format!(
            "mcap {} below minMcap {}",
            display_optional_f64(mcap),
            display_f64(s.min_mcap)
        ));
    }
    if mcap.is_some_and(|mcap| mcap > s.max_mcap) {
        return Some(format!(
            "mcap {} above maxMcap {}",
            display_optional_f64(mcap),
            display_f64(s.max_mcap)
        ));
    }
    if holders.is_none_or(|holders| holders < s.min_holders) {
        return Some(format!(
            "holders {} below minHolders {}",
            holders
                .map(|holders| holders.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            s.min_holders
        ));
    }
    if pool.volume.is_none_or(|volume| volume < s.min_volume) {
        return Some(format!(
            "volume {} below minVolume {}",
            display_optional_f64(pool.volume),
            display_f64(s.min_volume)
        ));
    }
    if tvl.is_none_or(|tvl| tvl < s.min_tvl) {
        return Some(format!(
            "TVL {} below minTvl {}",
            display_optional_f64(tvl),
            display_f64(s.min_tvl)
        ));
    }
    if let Some(max_tvl) = s.max_tvl {
        if tvl.is_some_and(|tvl| tvl > max_tvl) {
            return Some(format!(
                "TVL {} above maxTvl {}",
                display_optional_f64(tvl),
                display_f64(max_tvl)
            ));
        }
    }
    if bin_step.is_some_and(|bin_step| bin_step < s.min_bin_step) {
        return Some(format!(
            "bin_step {} below minBinStep {}",
            bin_step
                .map(|bin_step| bin_step.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            s.min_bin_step
        ));
    }
    if bin_step.is_some_and(|bin_step| bin_step > s.max_bin_step) {
        return Some(format!(
            "bin_step {} above maxBinStep {}",
            bin_step.unwrap_or_default(),
            s.max_bin_step
        ));
    }
    if fee_ratio.is_none_or(|fee_ratio| fee_ratio < s.min_fee_active_tvl_ratio) {
        return Some(format!(
            "fee/active-TVL {} below minFeeActiveTvlRatio {}",
            display_optional_f64(fee_ratio),
            display_f64(s.min_fee_active_tvl_ratio)
        ));
    }
    if volatility.is_none_or(|volatility| volatility <= 0.0) {
        return Some(format!(
            "volatility {} is unusable",
            display_optional_f64(volatility)
        ));
    }
    if base_organic.is_none_or(|organic| organic < s.min_organic) {
        return Some(format!(
            "base organic {} below minOrganic {}",
            display_optional_f64(base_organic),
            display_f64(s.min_organic)
        ));
    }
    if quote_organic.is_none_or(|organic| organic < s.min_quote_organic) {
        return Some(format!(
            "quote organic {} below minQuoteOrganic {}",
            display_optional_f64(quote_organic),
            display_f64(s.min_quote_organic)
        ));
    }
    if is_discord_signal(pool)
        && !s.allowed_launchpads.is_empty()
        && launchpad
            .as_deref()
            .is_some_and(|lp| !includes_case_insensitive(&s.allowed_launchpads, lp))
    {
        return Some(format!(
            "launchpad {} not in allow-list",
            launchpad.unwrap_or_default()
        ));
    }
    if launchpad
        .as_deref()
        .is_some_and(|lp| includes_case_insensitive(&s.blocked_launchpads, lp))
    {
        return Some(format!(
            "blocked launchpad ({})",
            launchpad.unwrap_or_default()
        ));
    }
    if let Some(hours) = s.min_token_age_hours {
        let max_created_at = chrono::Utc::now().timestamp_millis() as f64 - hours * 3_600_000.0;
        if created_at.is_none_or(|created_at| created_at > max_created_at) {
            return Some(format!("token age below minTokenAgeHours {hours}"));
        }
    }
    if let Some(hours) = s.max_token_age_hours {
        let min_created_at = chrono::Utc::now().timestamp_millis() as f64 - hours * 3_600_000.0;
        if created_at.is_some_and(|created_at| created_at < min_created_at) {
            return Some(format!("token age above maxTokenAgeHours {hours}"));
        }
    }
    None
}

fn pool_launchpad(pool: &RawPool) -> Option<String> {
    pool.token_x
        .as_ref()
        .and_then(|base| {
            base.launchpad
                .clone()
                .or_else(|| base.launchpad_platform.clone())
        })
        .or_else(|| pool.base_token_launchpad.clone())
        .or_else(|| extra_string(pool, "launchpad"))
        .or_else(|| extra_string(pool, "launchpad_platform"))
}

fn extra_string(pool: &RawPool, key: &str) -> Option<String> {
    pool.extra
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_discord_signal(pool: &RawPool) -> bool {
    pool.extra
        .get("discord_signal")
        .and_then(|value| value.as_bool())
        == Some(true)
}

fn includes_case_insensitive(values: &[String], value: &str) -> bool {
    values.iter().any(|entry| entry.eq_ignore_ascii_case(value))
}

fn display_optional_f64(value: Option<f64>) -> String {
    value
        .map(display_f64)
        .unwrap_or_else(|| "unknown".to_string())
}

fn display_f64(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

pub fn apply_pvp_risk_policy(candidates: &mut Vec<CondensedPool>, s: &ScreeningConfig) {
    let mut ignored = Vec::new();
    apply_pvp_risk_policy_with_rejections(candidates, s, &mut ignored);
}

pub fn apply_pvp_risk_policy_with_rejections(
    candidates: &mut Vec<CondensedPool>,
    s: &ScreeningConfig,
    filtered_examples: &mut Vec<FilteredPoolExample>,
) {
    if !s.avoid_pvp_symbols || candidates.is_empty() {
        return;
    }

    enrich_pvp_risk_from_candidate_set(candidates);

    if s.block_pvp_symbols {
        let before = candidates.len();
        let mut kept = Vec::with_capacity(candidates.len());
        for candidate in candidates.drain(..) {
            if candidate.is_pvp == Some(true) {
                filtered_examples.push(FilteredPoolExample {
                    name: candidate.name.clone(),
                    reason: "PVP hard filter".to_string(),
                });
            } else {
                kept.push(candidate);
            }
        }
        *candidates = kept;
        let removed = before.saturating_sub(candidates.len());
        if removed > 0 {
            info(
                "screening",
                &format!("PVP hard filter removed {removed} pool(s)"),
            );
        }
    }
}

fn enrich_pvp_risk_from_candidate_set(candidates: &mut [CondensedPool]) {
    let snapshot = candidates.to_vec();

    for candidate in candidates.iter_mut() {
        let symbol = normalize_symbol(&candidate.base.symbol);
        if symbol.is_empty() || candidate.base.mint.is_empty() {
            continue;
        }

        let rival = snapshot
            .iter()
            .filter(|rival| rival.pool_address != candidate.pool_address)
            .filter(|rival| rival.base.mint != candidate.base.mint)
            .filter(|rival| normalize_symbol(&rival.base.symbol) == symbol)
            .filter(|rival| is_meaningful_pvp_rival(rival))
            .max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        a.tvl
                            .partial_cmp(&b.tvl)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });

        if let Some(rival) = rival {
            candidate.is_pvp = Some(true);
            candidate.pvp_risk = Some("high".to_string());
            candidate.pvp_symbol = Some(symbol);
            candidate.pvp_rival_name = Some(rival.name.clone());
            candidate.pvp_rival_mint = Some(rival.base.mint.clone());
            candidate.pvp_rival_pool = Some(rival.pool_address.clone());
            candidate.pvp_rival_tvl = Some(rival.tvl.round());
            candidate.pvp_rival_holders = Some(rival.holders);
            candidate.pvp_rival_fees = Some((rival.fees_sol * 100.0).round() / 100.0);
        }
    }
}

fn is_meaningful_pvp_rival(pool: &CondensedPool) -> bool {
    pool.tvl >= PVP_MIN_ACTIVE_TVL
        && pool.holders >= PVP_MIN_HOLDERS
        && pool.fees_sol >= PVP_MIN_GLOBAL_FEES_SOL
}

fn normalize_symbol(symbol: &str) -> String {
    symbol.trim().to_uppercase()
}

fn score_candidate(pool: &RawPool) -> f64 {
    let fee_tvl = pool.fee_active_tvl_ratio.unwrap_or(0.0);
    let organic = pool
        .token_x
        .as_ref()
        .and_then(|t| t.organic_score)
        .or(pool.base_token_organic_score)
        .unwrap_or(0.0);
    let volume = pool.volume.unwrap_or(0.0);
    let holders = pool.base_token_holders.unwrap_or(0) as f64;
    fee_tvl * 1000.0 + organic * 10.0 + volume / 100.0 + holders / 100.0
}

fn get_volatility_timeframe(source: &str) -> &str {
    let source_min = TIMEFRAME_MINUTES
        .iter()
        .find(|(k, _)| *k == source)
        .map(|(_, v)| *v);
    let min_30m = 30u32;
    match source_min {
        Some(m) if m >= min_30m => source,
        _ => MIN_VOLATILITY_TIMEFRAME,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pool::PoolToken;

    fn candidate(
        pool_address: &str,
        mint: &str,
        symbol: &str,
        score: f64,
        tvl: f64,
        holders: u64,
        fees_sol: f64,
    ) -> CondensedPool {
        CondensedPool {
            name: format!("{symbol}/SOL"),
            pool_address: pool_address.to_string(),
            base: PoolToken {
                mint: mint.to_string(),
                symbol: symbol.to_string(),
                organic: 75.0,
                mcap: Some(500_000.0),
                icon: None,
            },
            quote_organic: 80.0,
            tvl,
            volume: 10_000.0,
            fee_active_tvl_ratio: 0.12,
            volatility: 0.5,
            bin_step: 100,
            score,
            holders,
            mcap: 500_000.0,
            fees_sol,
            launchpad: Some("meteora".to_string()),
            dev: None,
            bundlers_pct: None,
            top10_pct: None,
            discord_signal: None,
            is_pvp: None,
            pvp_risk: None,
            pvp_symbol: None,
            pvp_rival_name: None,
            pvp_rival_mint: None,
            pvp_rival_pool: None,
            pvp_rival_tvl: None,
            pvp_rival_holders: None,
            pvp_rival_fees: None,
            smart_money_count: None,
        }
    }

    #[test]
    fn pvp_policy_marks_exact_symbol_rivals_with_high_risk_metadata() {
        let mut screening = crate::config::types::Config::default().screening;
        screening.avoid_pvp_symbols = true;
        screening.block_pvp_symbols = false;
        let mut candidates = vec![
            candidate("PoolAlpha", "MintAlpha", "MOON", 500.0, 20_000.0, 900, 41.0),
            candidate("PoolBeta", "MintBeta", "MOON", 450.0, 15_000.0, 800, 35.0),
            candidate(
                "PoolGamma",
                "MintGamma",
                "MOONSHOT",
                900.0,
                30_000.0,
                1_200,
                50.0,
            ),
        ];

        apply_pvp_risk_policy(&mut candidates, &screening);

        let alpha = candidates
            .iter()
            .find(|pool| pool.pool_address == "PoolAlpha")
            .unwrap();
        assert_eq!(alpha.is_pvp, Some(true));
        assert_eq!(alpha.pvp_risk.as_deref(), Some("high"));
        assert_eq!(alpha.pvp_symbol.as_deref(), Some("MOON"));
        assert_eq!(alpha.pvp_rival_mint.as_deref(), Some("MintBeta"));
        assert_eq!(alpha.pvp_rival_pool.as_deref(), Some("PoolBeta"));
        assert_eq!(alpha.pvp_rival_holders, Some(800));
        assert_eq!(alpha.pvp_rival_fees, Some(35.0));

        let gamma = candidates
            .iter()
            .find(|pool| pool.pool_address == "PoolGamma")
            .unwrap();
        assert_eq!(gamma.is_pvp, None);
    }

    #[test]
    fn pvp_policy_hard_blocks_flagged_candidates_when_configured() {
        let mut screening = crate::config::types::Config::default().screening;
        screening.avoid_pvp_symbols = true;
        screening.block_pvp_symbols = true;
        let mut candidates = vec![
            candidate("PoolAlpha", "MintAlpha", "MOON", 500.0, 20_000.0, 900, 41.0),
            candidate("PoolBeta", "MintBeta", "MOON", 450.0, 15_000.0, 800, 35.0),
            candidate("PoolSafe", "MintSafe", "SAFE", 300.0, 12_000.0, 650, 34.0),
        ];

        apply_pvp_risk_policy(&mut candidates, &screening);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].pool_address, "PoolSafe");
    }

    #[test]
    fn pvp_metadata_serializes_for_llm_candidate_context() {
        let mut pool = candidate("PoolAlpha", "MintAlpha", "MOON", 500.0, 20_000.0, 900, 41.0);
        pool.is_pvp = Some(true);
        pool.pvp_risk = Some("high".to_string());
        pool.pvp_rival_name = Some("Moon Rival".to_string());
        pool.pvp_rival_mint = Some("MintBeta".to_string());

        let json = serde_json::to_value(pool).expect("candidate serializes");

        assert_eq!(json["is_pvp"], true);
        assert_eq!(json["pvp_risk"], "high");
        assert_eq!(json["pvp_rival_name"], "Moon Rival");
        assert_eq!(json["pvp_rival_mint"], "MintBeta");
    }

    #[test]
    fn single_candidate_skip_reason_requires_extra_conviction_for_weak_lone_candidate() {
        let weak_fees = candidate("PoolWeak", "MintWeak", "WEAK", 80.0, 20_000.0, 900, 12.0);

        assert_eq!(
            single_candidate_skip_reason(&weak_fees).as_deref(),
            Some("only one candidate survived and token fees are weak (12.00 SOL); original JS requires extra conviction before lone-candidate deploys")
        );

        let mut discord_confirmed = weak_fees.clone();
        discord_confirmed.discord_signal = Some(true);
        assert_eq!(single_candidate_skip_reason(&discord_confirmed), None);

        let mut pvp = candidate("PoolPvp", "MintPvp", "MOON", 500.0, 20_000.0, 900, 41.0);
        pvp.is_pvp = Some(true);
        pvp.pvp_rival_name = Some("Moon Rival".to_string());
        assert_eq!(
            single_candidate_skip_reason(&pvp).as_deref(),
            Some("only one candidate survived, but it has high PVP risk against Moon Rival")
        );
    }

    fn raw_pool(name: &str, pool_address: &str, mint: &str, symbol: &str) -> RawPool {
        RawPool {
            pool_address: Some(pool_address.to_string()),
            name: Some(name.to_string()),
            pool_type: Some("dlmm".to_string()),
            tvl: Some(20_000.0),
            active_tvl: None,
            volume: Some(10_000.0),
            fee: Some(40.0),
            fee_active_tvl_ratio: Some(0.5),
            volatility: Some(0.8),
            base_token_holders: Some(800),
            base_token_mcap: Some(500_000.0),
            base_token_organic_score: Some(75.0),
            quote_token_organic_score: Some(80.0),
            base_token_has_critical_warnings: Some(false),
            quote_token_has_critical_warnings: Some(false),
            base_token_has_high_supply_concentration: Some(false),
            base_token_has_high_single_ownership: Some(false),
            base_token_launchpad: None,
            dlmm_bin_step: Some(100),
            token_x: Some(crate::models::pool::TokenX {
                address: Some(mint.to_string()),
                symbol: Some(symbol.to_string()),
                name: Some(symbol.to_string()),
                market_cap: Some(500_000.0),
                organic_score: Some(75.0),
                launchpad: None,
                launchpad_platform: None,
                created_at: Some(0.0),
                icon: None,
            }),
            token_y: Some(crate::models::pool::TokenY {
                address: Some("So11111111111111111111111111111111111111112".to_string()),
                symbol: Some("SOL".to_string()),
                organic_score: Some(80.0),
            }),
            extra: Default::default(),
        }
    }

    #[test]
    fn reject_reason_reports_detailed_threshold_and_launchpad_reasons() {
        let mut screening = crate::config::types::Config::default().screening;
        screening.min_volume = 500.0;
        screening.blocked_launchpads = vec!["pump.fun".to_string()];
        screening.allowed_launchpads = vec!["meteora".to_string()];

        let mut low_volume = raw_pool("LowVol/SOL", "PoolLow", "MintLow", "LOW");
        low_volume.volume = None;
        assert_eq!(
            raw_pool_screening_reject_reason(&low_volume, &screening).as_deref(),
            Some("volume unknown below minVolume 500")
        );

        let mut blocked = raw_pool("Blocked/SOL", "PoolBlocked", "MintBlocked", "BLOCK");
        blocked.base_token_launchpad = Some("pump.fun".to_string());
        assert_eq!(
            raw_pool_screening_reject_reason(&blocked, &screening).as_deref(),
            Some("blocked launchpad (pump.fun)")
        );

        let mut discord_not_allowed = raw_pool("Moon/SOL", "PoolMoon", "MintMoon", "MOON");
        discord_not_allowed.base_token_launchpad = Some("moonshot".to_string());
        discord_not_allowed
            .extra
            .insert("discord_signal".to_string(), serde_json::json!(true));
        assert_eq!(
            raw_pool_screening_reject_reason(&discord_not_allowed, &screening).as_deref(),
            Some("launchpad moonshot not in allow-list")
        );
    }

    #[test]
    fn screen_raw_pools_collects_filtered_examples_with_reasons() {
        let mut screening = crate::config::types::Config::default().screening;
        screening.min_volume = 500.0;
        let mut low_volume = raw_pool("LowVol/SOL", "PoolLow", "MintLow", "LOW");
        low_volume.volume = None;

        let result = screen_raw_pools(vec![low_volume], &screening, 10);

        assert_eq!(result.total_screened, 1);
        assert!(result.candidates.is_empty());
        assert_eq!(result.filtered_examples.len(), 1);
        assert_eq!(result.filtered_examples[0].name, "LowVol/SOL");
        assert_eq!(
            result.filtered_examples[0].reason,
            "volume unknown below minVolume 500"
        );
    }
}
