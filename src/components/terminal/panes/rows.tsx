'use client';

import { TokenLogo } from '../TokenLogo';
import {
  SOL_ICON,
  logoSrcs,
  formatCompact,
  plainUsd,
  EVENT_COLORS,
  type PositionRow,
  type Candidate,
  type LogEntry,
  type PoolHistory,
} from '../../../lib/meridianFormat';

// ── Position row (open positions) ──────────────────────────────────────
export const PositionRowView = ({
  row,
  size = 'sm',
  onClose,
}: {
  row: PositionRow;
  size?: 'sm' | 'lg';
  onClose?: (row: PositionRow) => void;
}) => {
  const lg = size === 'lg';
  return (
    <div className={`mrd-pos-row ${lg ? 'mrd-pos-grid-lg' : 'mrd-pos-grid'}`}>
      <div style={{ minWidth: 0 }}>
        <div className={`mrd-range-head ${lg ? 'lg' : ''}`}>
          {row.range}
          <span className="ext">↗</span>
          <span className="mrd-range-state" style={{ color: row.inRange ? 'var(--green)' : 'var(--dim)' }}>{row.rangeState}</span>
        </div>
        <div className="mrd-range-meta"><span>{row.quote}</span><span className="mrd-sep">·</span><span>{row.age}</span></div>
        <div className={`mrd-track ${lg ? 'lg' : ''}`}>
          <div className="fill" />
          {row.markerPct !== null ? <span className="mark" style={{ left: `${row.markerPct}%` }} /> : null}
        </div>
        {lg ? (
          <div className="mrd-track-labels"><span>MIN</span><span>ACTIVE</span><span>MAX</span></div>
        ) : null}
      </div>

      <div className={`mrd-stack ${lg ? 'lg' : ''}`}>
        <span className="main">{row.liquidityUsd}</span>
        <div className="mrd-leg"><span className="sig base">{row.sigil}</span><span className="txt">{row.liquiditySecondary}</span></div>
        <div className="mrd-leg"><TokenLogo srcs={SOL_ICON ? [SOL_ICON] : []} alt="SOL" /><span className="txt">{row.liquidityPrimary}</span></div>
      </div>

      <div className={`mrd-stack ${lg ? 'lg' : ''}`}>
        <span className="main fees">{row.feesUsd}<em>{row.feesApr}</em></span>
        <div className="mrd-leg"><span className="sig base">{row.sigil}</span><span className="txt">{row.feesSecondary}</span></div>
        <div className="mrd-leg"><TokenLogo srcs={SOL_ICON ? [SOL_ICON] : []} alt="SOL" /><span className="txt">{row.feesPrimary}</span></div>
      </div>

      <div className={`mrd-pnl ${lg ? 'lg' : ''}`}>
        <span className={`amt ${row.pnlPositive ? 'up' : 'down'}`}>{row.pnlUsd}</span>
        <span className={`pct ${row.pnlPositive ? 'up' : 'down'}`}>{row.pnlPct}</span>
        {lg && onClose ? <button type="button" className="mrd-close" onClick={() => onClose(row)}>CLOSE</button> : null}
      </div>
    </div>
  );
};

// ── Candidate row (radar) ──────────────────────────────────────────────
export const CandidateRowView = ({
  candidate,
  barPct,
  size = 'sm',
}: {
  candidate: Candidate;
  barPct: number;
  size?: 'sm' | 'lg';
}) => {
  const lg = size === 'lg';
  const addr = candidate.pool_address ? `${candidate.pool_address.slice(0, 4)}…${candidate.pool_address.slice(-4)}` : '-';
  return (
    <div className={`mrd-cand-row ${lg ? 'mrd-cand-grid-lg' : 'mrd-cand-grid'}`}>
      <div className="mrd-cand-pair">
        <TokenLogo srcs={logoSrcs(candidate.base?.icon, candidate.base?.mint)} alt={candidate.base?.symbol ?? candidate.name ?? '?'} />
        <div style={{ minWidth: 0 }}>
          <div className="mrd-cand-name">{candidate.name ?? 'UNKNOWN'}{candidate.smart_money_count ? ` 🧠${candidate.smart_money_count}` : ''}</div>
          <div className="mrd-cand-pool">{addr}</div>
        </div>
      </div>
      <div className="mrd-cand-score">
        <span className="val">{formatCompact(candidate.score)}</span>
        <div className="mrd-cand-bar"><div className="fill" style={{ width: `${barPct}%` }} /></div>
      </div>
      <span className="mrd-num-r mrd-tvl">${formatCompact(candidate.tvl)}</span>
      <span className="mrd-num-r mrd-fees">◎{formatCompact(candidate.fees_sol)}</span>
    </div>
  );
};

// ── Activity log row (compact — overview) ──────────────────────────────
export const LogRowView = ({ log }: { log: LogEntry }) => (
  <div className="mrd-log-row">
    <span className="mrd-log-time">{log.time}</span>
    <span className="mrd-log-badge" style={{ color: EVENT_COLORS[log.kind] ?? EVENT_COLORS.info }}>{log.badge}</span>
    <div style={{ minWidth: 0 }}>
      <div className="mrd-log-msg" title={log.message}>{log.message}</div>
      {log.pair && log.pair !== '-' ? <div className="mrd-log-pair">{log.pair}</div> : null}
    </div>
  </div>
);

// ── Activity log row (full — log pane) ─────────────────────────────────
export const LogRowFullView = ({ log }: { log: LogEntry }) => (
  <div className="mrd-log-full-row">
    <span className="mrd-log-time">{log.time}</span>
    <span className="mrd-log-badge" style={{ color: EVENT_COLORS[log.kind] ?? EVENT_COLORS.info }}>{log.badge}</span>
    <span className="pair" title={log.pair}>{log.pair}</span>
    <span className="msg" title={log.message}>{log.message}</span>
  </div>
);

// ── Portfolio pool row ─────────────────────────────────────────────────
export const PortfolioRowView = ({ pool }: { pool: PoolHistory }) => {
  const win = Number(pool.pnlUsd ?? 0) >= 0;
  return (
    <div className="mrd-pf-row">
      <div className="mrd-pf-pool">
        <strong>{pool.poolName || 'UNKNOWN'}</strong>
        <small>{pool.closedCount ?? 0} closed</small>
      </div>
      <span className={`mrd-num-r ${win ? 'up' : 'down'}`} style={{ fontWeight: 700 }}>{plainUsd(pool.pnlUsd)}</span>
      <span className="mrd-num-r" style={{ color: 'var(--muted)' }}>{plainUsd(pool.depositUsd)}</span>
      <span className="mrd-num-r" style={{ color: 'var(--muted)' }}>{plainUsd(pool.withdrawUsd)}</span>
      <span className="mrd-num-r mrd-fees">{plainUsd(pool.feesUsd)}</span>
    </div>
  );
};
