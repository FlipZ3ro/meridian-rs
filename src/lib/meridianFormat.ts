// Shared formatting + data-mapping helpers for the Meridian terminal UI.
// Consolidated from the original widget components so every terminal pane maps
// the backend payloads identically (prices, PnL, fees, decisions, icons).

// ── Types ──────────────────────────────────────────────────────────────
export type BackendPosition = {
  id?: string;
  pool_address?: string;
  pool_name?: string | null;
  base_mint?: string | null;
  base_symbol?: string | null;
  lower_bin?: number;
  upper_bin?: number;
  amount_sol?: number;
  status?: string;
  created_at?: string;
  total_fees_claimed?: number;
  liquidity_sol?: number;
  liquidity_token?: number;
  claimable_fee_sol?: number;
  claimable_fee_token?: number;
  live_pnl_usd?: number;
  live_pnl_pct?: number;
  live_value_usd?: number;
  price_min?: number;
  price_max?: number;
  price_active?: number;
  fee_apr_pct?: number;
  in_range?: boolean;
  base_icon?: string | null;
  pnl_sol?: number | null;
  signal_snapshot?: {
    priceRange?: { min?: number; max?: number } | null;
    price_range?: { min?: number; max?: number } | null;
  } | null;
};

export type Candidate = {
  name?: string;
  pool_address?: string;
  score?: number;
  tvl?: number;
  volume?: number;
  fees_sol?: number;
  fee_active_tvl_ratio?: number;
  volatility?: number;
  base?: { mint?: string; symbol?: string; icon?: string };
  smart_money_count?: number;
};

export type PricingContext = {
  solUsd: number;
  tokenPrices: Record<string, number>;
  mintByPool: Record<string, string>;
};

export type PositionRow = {
  key: string;
  pair: string;
  sigil: string;
  range: string;
  quote: string;
  age: string;
  liquidityUsd: string;
  liquidityPrimary: string;
  liquiditySecondary: string;
  feesUsd: string;
  feesPrimary: string;
  feesSecondary: string;
  feesApr: string;
  pnlUsd: string;
  pnlPct: string;
  pnlPositive: boolean;
  status: string;
  rangeState: string;
  markerPct: number | null;
  inRange: boolean;
  baseIconSrcs: string[];
  posId: string;
};

export type PoolHistory = {
  pool?: string;
  poolName?: string;
  pnlUsd?: number;
  depositUsd?: number;
  withdrawUsd?: number;
  feesUsd?: number;
  closedCount?: number;
  winCount?: number;
};

export type Decision = {
  timestamp?: string;
  tool?: string;
  action?: string;
  type?: string;
  pair?: string;
  pool?: string;
  pool_name?: string;
  position?: string;
  args?: { pool?: string; pool_address?: string; position_id?: string };
  message?: string;
  reason?: string;
  summary?: string | Record<string, unknown>;
  resultSummary?: string;
  result?: string | Record<string, unknown>;
  success?: boolean;
};

export type LogEntry = { time: string; badge: string; kind: string; pair: string; message: string };

// ── Colour tokens (mirror terminal.css) ────────────────────────────────
export const EVENT_COLORS: Record<string, string> = {
  deploy: '#22c55e',
  close: '#f97316',
  claim: '#2dd4bf',
  screen: '#8b5cf6',
  swap: '#38bdf8',
  fail: '#ef4444',
  skip: '#c79a4e',
  info: '#7c84a3',
};

// ── Number / string formatting ─────────────────────────────────────────
export const formatUsd = (value: number) =>
  `${value < 0 ? '-$' : '$'}${Math.abs(value) >= 1000 ? `${(Math.abs(value) / 1000).toFixed(2)}K` : Math.abs(value).toFixed(2)}`;

export const signedUsd = (value: number) => {
  const n = Number(value ?? 0);
  return `${n >= 0 ? '+' : '-'}$${Math.abs(n).toFixed(2)}`;
};

export const plainUsd = (value?: number) => {
  const n = Number(value ?? 0);
  return `${n < 0 ? '-' : ''}$${Math.abs(n).toFixed(2)}`;
};

export const pctText = (value?: number) =>
  `${Number(value ?? 0) >= 0 ? '+' : ''}${Number(value ?? 0).toFixed(2)}%`;

export const formatCompact = (value?: number) => {
  if (value == null || !Number.isFinite(value)) return '-';
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toFixed(value >= 10 ? 0 : 2);
};

export const formatTokenAmount = (value: number) => {
  if (!Number.isFinite(value)) return '-';
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(2)}K`;
  if (Math.abs(value) >= 1) return value.toFixed(2);
  return value.toFixed(6);
};

const formatPrice = (value: number) => {
  if (!Number.isFinite(value) || value <= 0) return '-';
  if (value >= 1) return value.toFixed(2);
  if (value >= 0.001) return value.toFixed(5);
  return value.toExponential(2);
};

const SUBSCRIPT_DIGITS = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
const toSubscript = (n: number) => String(n).split('').map((d) => SUBSCRIPT_DIGITS[Number(d)]).join('');

// Meteora-style tiny-price notation: 0.0000141 -> "0.0₄141".
export const formatSubPrice = (value: number) => {
  if (!Number.isFinite(value) || value <= 0) return '-';
  if (value >= 1) return value.toPrecision(4).replace(/\.?0+$/, '');
  if (value >= 0.001) return value.toFixed(4);
  const fixed = value.toFixed(20);
  const match = fixed.match(/^0\.(0*)(\d+)/);
  if (!match) return value.toExponential(2);
  const zeros = match[1].length;
  const sig = match[2].replace(/0+$/, '').slice(0, 3) || '0';
  return `0.0${toSubscript(zeros)}${sig}`;
};

const formatPriceRange = (min?: number, max?: number) => {
  if (!Number.isFinite(min as number) || !Number.isFinite(max as number)) return null;
  return `${formatSubPrice(min as number)} - ${formatSubPrice(max as number)}`;
};

export const formatAge = (createdAt?: string) => {
  if (!createdAt) return '-';
  const created = new Date(createdAt).getTime();
  if (!Number.isFinite(created)) return '-';
  const minutes = Math.floor(Math.max(0, Date.now() - created) / 60000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
};

export const shortAddr = (value?: string) =>
  value && value.length > 8 ? `${value.slice(0, 4)}…${value.slice(-4)}` : value ?? '-';

export const sigilOf = (name?: string | null) =>
  ((name ?? '').split(/[-/ ]/)[0] || '?').slice(0, 2).toUpperCase();

// ── Token icons ────────────────────────────────────────────────────────
export const proxiedIcon = (url?: string | null) =>
  url ? `https://wsrv.nl/?url=${encodeURIComponent(url)}&w=32&h=32&fit=cover&output=webp` : null;

const tokenIconUrl = (mint?: string | null) =>
  mint ? `https://dd.dexscreener.com/ds-data/tokens/solana/${mint}.png?size=lg` : null;

export const SOL_ICON = proxiedIcon(
  'https://raw.githubusercontent.com/solana-labs/token-list/main/assets/mainnet/So11111111111111111111111111111111111111112/logo.png',
);

export const letterFor = (alt?: string | null) =>
  ((alt ?? '').split(/[-/ ]/)[0] || '?').slice(0, 1).toUpperCase();

export const logoSrcs = (icon?: string | null, mint?: string | null) =>
  [proxiedIcon(icon && icon.trim()), proxiedIcon(tokenIconUrl(mint))].filter(Boolean) as string[];

// ── Position mapping ───────────────────────────────────────────────────
const resolveMint = (position: BackendPosition, mintByPool: Record<string, string>) => {
  const fromPool = position.pool_address ? mintByPool[position.pool_address] : undefined;
  const mint = fromPool ?? position.base_mint ?? undefined;
  return mint && mint !== position.pool_address ? mint : undefined;
};

export const positionMint = resolveMint;

const rangeFromSnapshot = (position: BackendPosition) => {
  const range = position.signal_snapshot?.priceRange ?? position.signal_snapshot?.price_range;
  const min = Number(range?.min);
  const max = Number(range?.max);
  if (!Number.isFinite(min) || !Number.isFinite(max) || min <= 0 || max <= 0) return null;
  return `${formatPrice(min)} - ${formatPrice(max)}`;
};

const formatRange = (lower: number | undefined, upper: number | undefined, tokenUsd: number, solUsd: number) => {
  if ((!lower && !upper) && tokenUsd > 0 && solUsd > 0) {
    const tokenInSol = tokenUsd / solUsd;
    return `${formatPrice(tokenInSol * 0.8)} - ${formatPrice(tokenInSol * 1.4)}`;
  }
  return `${lower ?? '-'} - ${upper ?? '-'}`;
};

export const mapPosition = (position: BackendPosition, pricing: PricingContext): PositionRow => {
  const amountSol = Number(position.amount_sol ?? 0);
  const pnlSol = Number(position.pnl_sol ?? 0);
  const solUsd = pricing.solUsd;
  const mint = resolveMint(position, pricing.mintByPool);
  const tokenUsd = mint ? Number(pricing.tokenPrices[mint] ?? 0) : 0;

  const hasLiveLiquidity = position.liquidity_sol !== undefined;
  const solLeg = hasLiveLiquidity ? Number(position.liquidity_sol ?? 0) : amountSol / 2;
  const tokenLeg = hasLiveLiquidity
    ? Number(position.liquidity_token ?? 0)
    : (tokenUsd > 0 ? ((amountSol / 2) * solUsd) / tokenUsd : 0);
  const solLegUsd = solLeg * solUsd;
  const tokenLegUsd = tokenLeg * tokenUsd;
  const liquidityUsd = solLegUsd + tokenLegUsd;

  const hasLivePnl = position.live_pnl_pct !== undefined || position.live_pnl_usd !== undefined;
  const pnlUsd = hasLivePnl ? Number(position.live_pnl_usd ?? 0) : pnlSol * solUsd;
  const pnlPct = hasLivePnl
    ? Number(position.live_pnl_pct ?? 0)
    : (liquidityUsd > 0 ? (pnlUsd / liquidityUsd) * 100 : 0);

  const feeSolLeg = Number(position.claimable_fee_sol ?? 0);
  const feeTokenLeg = Number(position.claimable_fee_token ?? 0);
  const feeSolUsd = feeSolLeg * solUsd;
  const feeTokenUsd = feeTokenLeg * tokenUsd;
  const feesUsd = feeSolUsd + feeTokenUsd;
  const symbol = position.base_symbol ?? position.pool_name ?? 'TOKEN';

  const liveRange = formatPriceRange(position.price_min, position.price_max);
  const pMin = Number(position.price_min);
  const pMax = Number(position.price_max);
  const pActive = Number(position.price_active);
  const markerPct = (Number.isFinite(pMin) && Number.isFinite(pMax) && Number.isFinite(pActive) && pMax > pMin)
    ? Math.min(100, Math.max(0, ((pActive - pMin) / (pMax - pMin)) * 100))
    : null;
  const feeApr = position.fee_apr_pct !== undefined
    ? Number(position.fee_apr_pct)
    : (liquidityUsd > 0 ? (feesUsd / liquidityUsd) * 100 : 0);

  return {
    key: position.id ?? position.pool_address ?? position.base_mint ?? position.pool_name ?? 'position',
    pair: symbol,
    sigil: sigilOf(symbol),
    range: liveRange ?? rangeFromSnapshot(position) ?? formatRange(position.lower_bin, position.upper_bin, tokenUsd, solUsd),
    quote: `SOL per ${symbol}`,
    age: formatAge(position.created_at),
    liquidityUsd: formatUsd(liquidityUsd),
    liquidityPrimary: `${solLeg.toFixed(4)} SOL (${formatUsd(solLegUsd)})`,
    liquiditySecondary: tokenUsd > 0
      ? `${formatTokenAmount(tokenLeg)} ${symbol} (${formatUsd(tokenLegUsd)})`
      : `${formatTokenAmount(tokenLeg)} ${symbol}`,
    feesUsd: formatUsd(feesUsd),
    feesPrimary: `${feeSolLeg.toFixed(6)} SOL (${formatUsd(feeSolUsd)})`,
    feesSecondary: tokenUsd > 0
      ? `${formatTokenAmount(feeTokenLeg)} ${symbol} (${formatUsd(feeTokenUsd)})`
      : `${formatTokenAmount(feeTokenLeg)} ${symbol}`,
    feesApr: `${Math.max(0, feeApr).toFixed(2)}%`,
    pnlUsd: `${pnlUsd >= 0 ? '+' : '-'}${formatUsd(Math.abs(pnlUsd))}`,
    pnlPct: `${pnlPct >= 0 ? '+' : '-'}${Math.abs(pnlPct).toFixed(2)}%`,
    pnlPositive: pnlUsd >= 0,
    status: String(position.status ?? 'active').toUpperCase(),
    rangeState: (position.in_range ?? true) ? 'IN RANGE' : 'OUT',
    markerPct,
    inRange: position.in_range ?? true,
    baseIconSrcs: [
      proxiedIcon(position.base_icon),
      proxiedIcon(tokenIconUrl(mint ?? position.base_mint)),
    ].filter(Boolean) as string[],
    posId: position.id ?? '',
  };
};

// ── Decision (activity log) mapping ────────────────────────────────────
const asObject = (value: unknown): Record<string, any> | null => {
  if (!value) return null;
  if (typeof value === 'object') return value as Record<string, any>;
  if (typeof value === 'string') { try { return JSON.parse(value); } catch { return null; } }
  return null;
};

const num = (value: unknown, digits = 4) => {
  const n = Number(value);
  return Number.isFinite(n) ? n.toFixed(digits) : null;
};

const failReason = (decision: Decision): string => {
  const r =
    decision.resultSummary
    ?? (typeof decision.result === 'string' ? decision.result : '')
    ?? (typeof decision.summary === 'string' ? decision.summary : '')
    ?? decision.reason
    ?? '';
  return String(r);
};

const isSkip = (reason: string): boolean => {
  const r = reason.toLowerCase();
  return r.includes('safety check')
    || r.includes('%b')
    || r.includes('already have position')
    || r.includes('waiting for')
    || r.includes('over-extended')
    || r.includes('cooldown')
    || r.includes('not enough sol')
    || r.includes('not supported')
    || r.includes('skipped')
    || r.includes('skipping')
    || r.includes('decelerat')
    || r.includes('position_id required');
};

const eventOf = (decision: Decision): { label: string; kind: string } => {
  const tool = (decision.tool ?? decision.action ?? '').toLowerCase();
  if (decision.success === false) {
    return isSkip(failReason(decision)) ? { label: 'SKIP', kind: 'skip' } : { label: 'FAIL', kind: 'fail' };
  }
  if (tool.includes('deploy')) return { label: 'DEPLOY', kind: 'deploy' };
  if (tool.includes('close')) return { label: 'CLOSE', kind: 'close' };
  if (tool.includes('claim')) return { label: 'CLAIM', kind: 'claim' };
  if (tool.includes('swap')) return { label: 'SWAP', kind: 'swap' };
  if (tool.includes('screen')) return { label: 'SCREEN', kind: 'screen' };
  if (tool.includes('balance') || tool.includes('wallet')) return { label: 'INFO', kind: 'info' };
  return { label: 'OK', kind: 'info' };
};

const humanMessage = (decision: Decision): string => {
  if (decision.success === false) {
    const reason = failReason(decision)
      .replace(/^safety check failed:\s*/i, '')
      .replace(/\s*—\s*price not over-extended.*$/i, '')
      .replace(/,?\s*waiting for mean-reversion setup\.?$/i, '')
      .trim();
    if (reason) return reason;
  }
  const tool = (decision.tool ?? decision.action ?? '').toLowerCase();
  const data = asObject(decision.result) ?? asObject(decision.resultSummary) ?? asObject(decision.summary) ?? {};
  const name = decision.pool_name ?? (data.poolName as string) ?? '';

  if (tool.includes('balance') || tool.includes('wallet')) {
    const sol = num(data.sol ?? data.balanceSol);
    return sol ? `Wallet balance ${sol} SOL` : 'Checked wallet balance';
  }
  if (tool.includes('deploy')) {
    const amt = num(data.amountY ?? data.amount_sol, 3);
    return `Deployed ${name || 'position'}${amt ? ` · ${amt} SOL` : ''}`;
  }
  if (tool.includes('close')) return `Closed ${name || 'position'}`;
  if (tool.includes('claim')) {
    const fees = num(data.fees_claimed ?? data.claimable_fee_sol);
    return fees ? `Claimed ${fees} SOL fees${name ? ` · ${name}` : ''}` : `Claimed fees${name ? ` · ${name}` : ''}`;
  }
  if (tool.includes('swap')) return `Swapped${name ? ` · ${name}` : ''} to SOL`;
  if (tool.includes('screen')) return decision.reason || (data.note as string) || 'Screening cycle';
  if (decision.reason) return decision.reason;
  if (decision.message) return decision.message;
  if (!tool) return 'Backend action';
  const readVerb = /^(get|list|fetch|read)_/.test(tool);
  const label = tool.replace(/^(get|list|fetch|read)_/, '').replace(/_/g, ' ');
  const text = readVerb ? `Read ${label}` : label;
  return name ? `${text} · ${name}` : text;
};

export const mapDecision = (decision: Decision): LogEntry => {
  const { label, kind } = eventOf(decision);
  const pair = decision.pool_name
    ?? decision.pair
    ?? shortAddr(decision.pool ?? decision.args?.pool ?? decision.args?.pool_address ?? decision.position);
  return {
    time: formatAge(decision.timestamp),
    badge: label,
    kind,
    pair,
    message: humanMessage(decision),
  };
};
