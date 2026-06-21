'use client';

import { useEffect, useState } from 'react';
import { ExternalLink, Layers } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type BackendPosition = {
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
  pnl_sol?: number | null;
  signal_snapshot?: {
    priceRange?: { min?: number; max?: number } | null;
    price_range?: { min?: number; max?: number } | null;
  } | null;
};

type Candidate = {
  pool_address?: string;
  base?: { mint?: string; symbol?: string };
};

type PricingContext = {
  solUsd: number;
  tokenPrices: Record<string, number>;
  mintByPool: Record<string, string>;
};

type PositionRow = {
  key: string;
  pair: string;
  range: string;
  quote: string;
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
  age: string;
  markerPct: number | null;
  inRange: boolean;
};

const fallbackPositions: PositionRow[] = [];

const formatAge = (createdAt?: string) => {
  if (!createdAt) return '-';
  const created = new Date(createdAt).getTime();
  if (!Number.isFinite(created)) return '-';
  const diff = Math.max(0, Date.now() - created);
  const minutes = Math.floor(diff / 60000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
};

const formatUsd = (value: number) => `$${Math.abs(value) >= 1000 ? `${(value / 1000).toFixed(2)}K` : value.toFixed(2)}`;

const formatTokenAmount = (value: number) => {
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

// Meteora-style price formatting: tiny numbers collapse leading zeros into a
// subscript count, e.g. 0.0000141 -> "0.0₄141".
const formatSubPrice = (value: number) => {
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

const formatRange = (lower: number | undefined, upper: number | undefined, tokenUsd: number, solUsd: number) => {
  if ((!lower && !upper) && tokenUsd > 0 && solUsd > 0) {
    const tokenInSol = tokenUsd / solUsd;
    return `${formatPrice(tokenInSol * 0.8)} - ${formatPrice(tokenInSol * 1.4)}`;
  }
  return `${lower ?? '-'} - ${upper ?? '-'}`;
};

const rangeFromSnapshot = (position: BackendPosition) => {
  const range = position.signal_snapshot?.priceRange ?? position.signal_snapshot?.price_range;
  const min = Number(range?.min);
  const max = Number(range?.max);
  if (!Number.isFinite(min) || !Number.isFinite(max) || min <= 0 || max <= 0) return null;
  return `${formatPrice(min)} - ${formatPrice(max)}`;
};

const resolveMint = (position: BackendPosition, mintByPool: Record<string, string>) => {
  const fromPool = position.pool_address ? mintByPool[position.pool_address] : undefined;
  const mint = fromPool ?? position.base_mint ?? undefined;
  return mint && mint !== position.pool_address ? mint : undefined;
};

const mapPosition = (position: BackendPosition, pricing: PricingContext): PositionRow => {
  const amountSol = Number(position.amount_sol ?? 0);
  const pnlSol = Number(position.pnl_sol ?? 0);
  const solUsd = pricing.solUsd;
  const mint = resolveMint(position, pricing.mintByPool);
  const tokenUsd = mint ? Number(pricing.tokenPrices[mint] ?? 0) : 0;
  // Live on-chain liquidity legs (from the backend close quote) when present;
  // fall back to splitting the deploy amount when not enriched (e.g. dry-run).
  const hasLiveLiquidity = position.liquidity_sol !== undefined;
  const solLeg = hasLiveLiquidity ? Number(position.liquidity_sol ?? 0) : amountSol / 2;
  const tokenLeg = hasLiveLiquidity
    ? Number(position.liquidity_token ?? 0)
    : (tokenUsd > 0 ? ((amountSol / 2) * solUsd) / tokenUsd : 0);
  const solLegUsd = solLeg * solUsd;
  const tokenLegUsd = tokenLeg * tokenUsd;
  const liquidityUsd = solLegUsd + tokenLegUsd;
  // Live PnL from the backend (Meteora API: deposits + IL + fees) when present;
  // fall back to the stored pnl_sol estimate otherwise.
  const hasLivePnl = position.live_pnl_pct !== undefined || position.live_pnl_usd !== undefined;
  const pnlUsd = hasLivePnl ? Number(position.live_pnl_usd ?? 0) : pnlSol * solUsd;
  const pnlPct = hasLivePnl
    ? Number(position.live_pnl_pct ?? 0)
    : (liquidityUsd > 0 ? (pnlUsd / liquidityUsd) * 100 : 0);
  // Live claimable (pending) fees from the backend's on-chain quote — the SOL
  // leg and base-token leg are reported separately, not split from a total.
  const feeSolLeg = Number(position.claimable_fee_sol ?? 0);
  const feeTokenLeg = Number(position.claimable_fee_token ?? 0);
  const feeSolUsd = feeSolLeg * solUsd;
  const feeTokenUsd = feeTokenLeg * tokenUsd;
  const feesUsd = feeSolUsd + feeTokenUsd;
  const symbol = position.base_symbol ?? position.pool_name ?? 'TOKEN';

  // Real price range (Meteora subscript style) + active-price marker position.
  const liveRange = formatPriceRange(position.price_min, position.price_max);
  const pMin = Number(position.price_min);
  const pMax = Number(position.price_max);
  const pActive = Number(position.price_active);
  const markerPct = (Number.isFinite(pMin) && Number.isFinite(pMax) && Number.isFinite(pActive) && pMax > pMin)
    ? Math.min(100, Math.max(0, ((pActive - pMin) / (pMax - pMin)) * 100))
    : null;
  // Fee badge: prefer the 24h fee/TVL APR (what Meteora shows) over a raw ratio.
  const feeApr = position.fee_apr_pct !== undefined
    ? Number(position.fee_apr_pct)
    : (liquidityUsd > 0 ? (feesUsd / liquidityUsd) * 100 : 0);

  return {
    key: position.id ?? position.pool_name ?? Math.random().toString(36),
    pair: symbol,
    range: liveRange ?? rangeFromSnapshot(position) ?? formatRange(position.lower_bin, position.upper_bin, tokenUsd, solUsd),
    quote: `SOL per ${symbol}`,
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
    pnlUsd: `${pnlUsd >= 0 ? '+' : '-'}${formatUsd(pnlUsd)}`,
    pnlPct: `${pnlPct >= 0 ? '+' : '-'}${Math.abs(pnlPct).toFixed(2)}%`,
    pnlPositive: pnlUsd >= 0,
    status: String(position.status ?? 'active').toUpperCase(),
    age: formatAge(position.created_at),
    markerPct,
    inRange: position.in_range ?? true,
  };
};

export const PositionTable = () => {
  const [positions, setPositions] = useState<PositionRow[]>(fallbackPositions);

  useEffect(() => {
    let isMounted = true;

    const loadPositions = async () => {
      try {
        const [payload, candidatesPayload] = await Promise.all([
          cachedJson<any>('/api/meridian/positions', 8_000),
          cachedJson<any>('/api/meridian/candidates?limit=40', 60_000),
        ]);
        const positionsPayload = Array.isArray(payload?.data?.positions)
          ? payload.data.positions as BackendPosition[]
          : [];
        // "OPEN POSITIONS" should only list open positions. The backend returns
        // the full history (active + closed), so drop anything closed — otherwise
        // the count and rows include stale, already-exited positions.
        const openPositions = positionsPayload.filter(
          (position) => String(position.status ?? 'active').toLowerCase() !== 'closed',
        );
        const candidates = Array.isArray(candidatesPayload?.data?.candidates)
          ? candidatesPayload.data.candidates as Candidate[]
          : [];
        const mintByPool = Object.fromEntries(candidates
          .filter((candidate) => candidate.pool_address && candidate.base?.mint)
          .map((candidate) => [candidate.pool_address as string, candidate.base?.mint as string]));
        const mints = [...new Set(openPositions
          .map((position) => resolveMint(position, mintByPool))
          .filter(Boolean) as string[])];
        const prices = await cachedJson<any>(`/api/prices?mints=${encodeURIComponent(mints.join(','))}`, 30_000).catch(() => ({}));
        const pricing: PricingContext = {
          solUsd: Number(prices?.solUsd ?? 0),
          tokenPrices: prices?.tokenPrices ?? {},
          mintByPool,
        };
        const nextPositions = Array.isArray(payload?.data?.positions)
          ? openPositions.map((position) => mapPosition(position, pricing))
          : fallbackPositions;
        if (isMounted) setPositions(nextPositions);
      } catch {
        if (isMounted) setPositions(fallbackPositions);
      }
    };

    loadPositions();
    const timer = window.setInterval(loadPositions, 10_000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <GlassCard className="positions-card terminal-positions">
      <div className="card-title">
        <div><Layers size={24} /><h2>OPEN POSITIONS</h2></div>
        <span>{positions.length} POSITIONS</span>
      </div>
      <div className="position-head">
        <span>Price Range</span><span>Your Liquidity</span><span>Claimable Fees</span><span>PnL</span>
      </div>
      <div className="position-rows">
        {positions.length ? positions.map((position) => (
          <div className="meteora-position-row" key={position.key}>
            <div className="mp-range">
              <div className="mp-range-value">{position.range}<ExternalLink size={13} /></div>
              <div className="mp-meta"><span>{position.quote}</span><b>•</b><span>{position.age}</span></div>
              {position.markerPct !== null ? (
                <div
                  className="mp-range-track"
                  style={{
                    position: 'relative',
                    height: 6,
                    width: 150,
                    marginTop: 8,
                    borderRadius: 3,
                    background: position.inRange
                      ? 'linear-gradient(90deg, #2dd4bf, #8b5cf6)'
                      : 'linear-gradient(90deg, #475569, #64748b)',
                    opacity: 0.9,
                  }}
                >
                  <span
                    style={{
                      position: 'absolute',
                      top: -3,
                      left: `${position.markerPct}%`,
                      transform: 'translateX(-50%)',
                      width: 3,
                      height: 12,
                      borderRadius: 2,
                      background: '#fff',
                      boxShadow: '0 0 6px rgba(255,255,255,0.85)',
                    }}
                  />
                </div>
              ) : (
                <div className="mp-spark"><span /></div>
              )}
            </div>
            <div className="mp-stack">
              <div className="mp-main">{position.liquidityUsd}</div>
              <div className="mp-token"><i className="mp-dot mp-sol" /><span>{position.liquidityPrimary}</span></div>
              <div className="mp-token"><i className="mp-dot mp-pair" /><span>{position.liquiditySecondary}</span></div>
            </div>
            <div className="mp-stack">
              <div className="mp-main mp-fees-main">{position.feesUsd}<em>{position.feesApr}</em></div>
              <div className="mp-token"><i className="mp-dot mp-sol" /><span>{position.feesPrimary}</span></div>
              <div className="mp-token"><i className="mp-dot mp-pair" /><span>{position.feesSecondary}</span></div>
            </div>
            <div className={position.pnlPositive ? 'mp-pnl mp-up' : 'mp-pnl mp-down'}>
              <div>{position.pnlUsd}</div>
              <span>{position.pnlPct}</span>
            </div>
          </div>
        )) : <div className="positions-empty">No active backend positions.</div>}
      </div>
      <div className="fees-line"><span>/api/meridian/positions</span><span>backend live</span></div>
    </GlassCard>
  );
};
