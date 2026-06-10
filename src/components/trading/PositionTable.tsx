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
  const feesSol = Number(position.total_fees_claimed ?? 0);
  const pnlSol = Number(position.pnl_sol ?? 0);
  const solUsd = pricing.solUsd;
  const mint = resolveMint(position, pricing.mintByPool);
  const tokenUsd = mint ? Number(pricing.tokenPrices[mint] ?? 0) : 0;
  const liquidityUsd = amountSol * solUsd;
  const feesUsd = feesSol * solUsd;
  const pnlUsd = pnlSol * solUsd;
  const pnlPct = liquidityUsd > 0 ? (pnlUsd / liquidityUsd) * 100 : 0;
  const solLeg = amountSol / 2;
  const tokenLegUsd = Math.max(0, liquidityUsd - (solLeg * solUsd));
  const tokenLeg = tokenUsd > 0 ? tokenLegUsd / tokenUsd : 0;
  const feeSolLeg = feesSol / 2;
  const feeTokenUsd = Math.max(0, feesUsd - (feeSolLeg * solUsd));
  const feeTokenLeg = tokenUsd > 0 ? feeTokenUsd / tokenUsd : 0;
  const symbol = position.base_symbol ?? position.pool_name ?? 'TOKEN';

  return {
    key: position.id ?? position.pool_name ?? Math.random().toString(36),
    pair: symbol,
    range: rangeFromSnapshot(position) ?? formatRange(position.lower_bin, position.upper_bin, tokenUsd, solUsd),
    quote: `SOL per ${symbol}`,
    liquidityUsd: formatUsd(liquidityUsd),
    liquidityPrimary: `${solLeg.toFixed(4)} SOL (${formatUsd(solLeg * solUsd)})`,
    liquiditySecondary: tokenUsd > 0
      ? `${formatTokenAmount(tokenLeg)} ${symbol} (${formatUsd(tokenLegUsd)})`
      : `${symbol} price unavailable`,
    feesUsd: formatUsd(feesUsd),
    feesPrimary: `${feeSolLeg.toFixed(6)} SOL (${formatUsd(feeSolLeg * solUsd)})`,
    feesSecondary: tokenUsd > 0
      ? `${formatTokenAmount(feeTokenLeg)} ${symbol} (${formatUsd(feeTokenUsd)})`
      : `${symbol} price unavailable`,
    feesApr: `${Math.min(99.99, Math.max(0.01, Math.abs(feesSol) * 52000)).toFixed(2)}%`,
    pnlUsd: `${pnlUsd >= 0 ? '+' : '-'}${formatUsd(pnlUsd)}`,
    pnlPct: `${pnlPct >= 0 ? '+' : '-'}${Math.abs(pnlPct).toFixed(2)}%`,
    pnlPositive: pnlUsd >= 0,
    status: String(position.status ?? 'active').toUpperCase(),
    age: formatAge(position.created_at),
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
        const candidates = Array.isArray(candidatesPayload?.data?.candidates)
          ? candidatesPayload.data.candidates as Candidate[]
          : [];
        const mintByPool = Object.fromEntries(candidates
          .filter((candidate) => candidate.pool_address && candidate.base?.mint)
          .map((candidate) => [candidate.pool_address as string, candidate.base?.mint as string]));
        const mints = [...new Set(positionsPayload
          .map((position) => resolveMint(position, mintByPool))
          .filter(Boolean) as string[])];
        const prices = await cachedJson<any>(`/api/prices?mints=${encodeURIComponent(mints.join(','))}`, 30_000).catch(() => ({}));
        const pricing: PricingContext = {
          solUsd: Number(prices?.solUsd ?? 0),
          tokenPrices: prices?.tokenPrices ?? {},
          mintByPool,
        };
        const nextPositions = Array.isArray(payload?.data?.positions)
          ? positionsPayload.map((position) => mapPosition(position, pricing))
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
              <div className="mp-spark"><span /></div>
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
