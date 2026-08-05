'use client';

import { useEffect, useState } from 'react';
import { History } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type PoolHistory = {
  pool?: string;
  poolName?: string;
  pnlUsd?: number;
  depositUsd?: number;
  withdrawUsd?: number;
  feesUsd?: number;
  closedCount?: number;
  winCount?: number;
};

type Summary = {
  totalPnlUsd?: number;
  totalPnlPct?: number;
  allTimeDepositUsd?: number;
  feesClaimedUsd?: number;
  closedCount?: number;
  winRate?: number;
  avgInvestedUsd?: number;
};

const usd = (value?: number) => {
  const n = Number(value ?? 0);
  const sign = n < 0 ? '-' : '';
  return `${sign}$${Math.abs(n).toFixed(2)}`;
};

const pct = (value?: number) => `${Number(value ?? 0) >= 0 ? '+' : ''}${Number(value ?? 0).toFixed(2)}%`;

export const PortfolioWidget = () => {
  const [summary, setSummary] = useState<Summary>({});
  const [pools, setPools] = useState<PoolHistory[]>([]);
  const [note, setNote] = useState('Loading history…');

  useEffect(() => {
    let isMounted = true;
    const load = async () => {
      try {
        const payload = await cachedJson<any>('/api/meridian/portfolio', 60_000);
        const nextPools = Array.isArray(payload?.data?.pools) ? (payload.data.pools as PoolHistory[]) : [];
        const nextSummary = (payload?.data?.summary ?? {}) as Summary;
        if (isMounted) {
          setPools(nextPools);
          setSummary(nextSummary);
          setNote(nextPools.length ? `${nextSummary.closedCount ?? 0} closed positions` : 'No closed positions yet');
        }
      } catch {
        if (isMounted) {
          setPools([]);
          setNote('Backend unavailable');
        }
      }
    };
    load();
    const timer = window.setInterval(load, 60_000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  const pnlPositive = Number(summary.totalPnlUsd ?? 0) >= 0;

  return (
    <GlassCard className="positions-card terminal-positions">
      <div className="card-title">
        <div><History size={22} /><h2>HISTORICAL — DLMM POSITIONS</h2></div>
        <span>{summary.closedCount ?? 0} closed</span>
      </div>

      <div className="portfolio-summary">
        <div className="ps-stat">
          <span>Total PnL</span>
          <b className={pnlPositive ? 'profit' : 'loss'}>{usd(summary.totalPnlUsd)} <em>{pct(summary.totalPnlPct)}</em></b>
        </div>
        <div className="ps-stat"><span>Deposit</span><b>{usd(summary.allTimeDepositUsd)}</b></div>
        <div className="ps-stat"><span>Fees</span><b>{usd(summary.feesClaimedUsd)}</b></div>
        <div className="ps-stat"><span>Win Rate</span><b>{Number(summary.winRate ?? 0).toFixed(1)}%</b></div>
        <div className="ps-stat"><span>Avg Inv</span><b>{usd(summary.avgInvestedUsd)}</b></div>
      </div>

      <div className="portfolio-head">
        <span>Pool</span><span>PnL</span><span>Deposit</span><span>Withdraw</span><span>Fees Earned</span>
      </div>
      <div className="portfolio-rows">
        {pools.length ? pools.map((pool) => {
          const win = Number(pool.pnlUsd ?? 0) >= 0;
          return (
            <div className="portfolio-row" key={pool.pool ?? pool.poolName}>
              <div className="pr-pool">
                <strong>{pool.poolName || 'UNKNOWN'}</strong>
                <small>{pool.closedCount ?? 0} closed</small>
              </div>
              <span className={win ? 'profit' : 'loss'}>{usd(pool.pnlUsd)}</span>
              <span>{usd(pool.depositUsd)}</span>
              <span>{usd(pool.withdrawUsd)}</span>
              <span className="profit">{usd(pool.feesUsd)}</span>
            </div>
          );
        }) : <div className="positions-empty">{note}</div>}
      </div>
    </GlassCard>
  );
};
