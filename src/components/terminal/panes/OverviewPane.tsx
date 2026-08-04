'use client';

import { useState } from 'react';
import { usePositions, usePortfolio, useCandidates, useDecisions } from '../hooks';
import { PositionRowView, CandidateRowView, LogRowView, PortfolioRowView, PaneState } from './rows';
import { plainUsd } from '../../../lib/meridianFormat';

const barFor = (score: number | undefined, max: number) =>
  Math.max(4, Math.round(((Number(score ?? 0)) / (max || 1)) * 100));

export const OverviewPane = ({ filter = '' }: { filter?: string }) => {
  const { positions, loading: positionsLoading } = usePositions();
  const { summary, pools, note: portfolioNote, loading: portfolioLoading } = usePortfolio();
  const { candidates, note: radarNote, loading: radarLoading } = useCandidates(40);
  const { logs, loading: logsLoading } = useDecisions();
  const [tab, setTab] = useState<'open' | 'closed'>('open');

  const f = filter.trim().toLowerCase();
  const radar = candidates.filter((c) => !f || `${c.name ?? ''} ${c.pool_address ?? ''}`.toLowerCase().includes(f)).slice(0, 8);
  const maxScore = Math.max(...candidates.map((c) => Number(c.score ?? 0)), 1);
  const logRows = logs.filter((l) => !f || `${l.badge} ${l.pair} ${l.message}`.toLowerCase().includes(f));
  const closedRows = pools.filter((p) => !f || (p.poolName ?? '').toLowerCase().includes(f)).slice(0, 8);

  return (
    <div className="mrd-overview">
      <div className="mrd-ov-left">
        {/* Positions panel with open/closed tabs */}
        <section className="mrd-panel" aria-label="Positions">
          <span className="mrd-panel-label">POSITIONS</span>
          <div className="mrd-postabs" role="tablist" aria-label="Position state">
            <button type="button" role="tab" aria-selected={tab === 'open'} className={`mrd-postab ${tab === 'open' ? 'active' : ''}`} onClick={() => setTab('open')}>
              OPEN<span className="count">{positions.length}</span>
            </button>
            <button type="button" role="tab" aria-selected={tab === 'closed'} className={`mrd-postab ${tab === 'closed' ? 'active' : ''}`} onClick={() => setTab('closed')}>
              CLOSED<span className="count">{summary.closedCount ?? 0}</span>
            </button>
            <div className="mrd-postabs-fill" />
            <div className="mrd-postabs-note">SOL PER TOKEN</div>
          </div>

          {tab === 'open' ? (
            <>
              <div className="mrd-pos-head mrd-pos-grid">
                <span>PRICE RANGE</span><span>YOUR LIQUIDITY</span><span>CLAIMABLE FEES</span><span className="mrd-num-r">PNL</span>
              </div>
              {positions.length
                ? positions.map((p) => <PositionRowView key={p.key} row={p} size="sm" />)
                : <PaneState loading={positionsLoading} message="No active backend positions." rows={2} />}
            </>
          ) : (
            <>
              <div className="mrd-pf-summary">
                <div className="cell"><span className="k">TOTAL PNL</span><span className={`v ${Number(summary.totalPnlUsd ?? 0) >= 0 ? 'up' : 'down'}`} style={{ fontSize: 15 }}>{plainUsd(summary.totalPnlUsd)}</span></div>
                <div className="cell"><span className="k">DEPOSIT</span><span className="v" style={{ fontSize: 15 }}>{plainUsd(summary.allTimeDepositUsd)}</span></div>
                <div className="cell"><span className="k">FEES</span><span className="v mrd-fees" style={{ fontSize: 15 }}>{plainUsd(summary.feesClaimedUsd)}</span></div>
                <div className="cell"><span className="k">WIN RATE</span><span className="v" style={{ fontSize: 15 }}>{Number(summary.winRate ?? 0).toFixed(1)}%</span></div>
              </div>
              <div className="mrd-pf-head">
                <span>POOL</span><span className="mrd-num-r">PNL</span><span className="mrd-num-r">DEPOSIT</span><span className="mrd-num-r">WITHDRAW</span><span className="mrd-num-r">FEES</span>
              </div>
              {closedRows.length
                ? closedRows.map((p) => <PortfolioRowView key={p.pool ?? p.poolName} pool={p} />)
                : <PaneState loading={portfolioLoading} message={portfolioNote} rows={4} widths={['30%', '14%', '16%', '16%', '14%']} />}
            </>
          )}
        </section>

        {/* Candidate radar (compact) */}
        <section className="mrd-panel mrd-radar" aria-label="Candidate radar">
          <span className="mrd-panel-label">CANDIDATE RADAR</span>
          <span className="mrd-panel-right">{radar.length} PASSED</span>
          <div className="mrd-cand-head mrd-cand-grid">
            <span>PAIR</span><span>SCORE</span><span className="mrd-num-r">TVL</span><span className="mrd-num-r">FEES</span>
          </div>
          <div className="mrd-cand-body">
            {radar.length
              ? radar.map((c) => <CandidateRowView key={c.pool_address ?? c.name} candidate={c} barPct={barFor(c.score, maxScore)} size="sm" />)
              : <PaneState loading={radarLoading} message={radarNote} rows={3} />}
          </div>
        </section>
      </div>

      {/* Activity log (tail -f) */}
      <aside className="mrd-ov-right mrd-log-panel" aria-label="Activity log">
        <span className="mrd-panel-label">ACTIVITY LOG</span>
        <span className="mrd-panel-right">TAIL -F</span>
        <div className="mrd-log-body">
          {logRows.length
            ? logRows.map((l, i) => <LogRowView key={`${l.time}-${i}`} log={l} />)
            : <PaneState loading={logsLoading} message="No backend decisions yet." rows={6} widths={['26%', '22%', '38%']} />}
        </div>
      </aside>
    </div>
  );
};
