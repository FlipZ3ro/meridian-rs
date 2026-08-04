'use client';

import { usePortfolio } from '../hooks';
import { PortfolioRowView, PaneState } from './rows';
import { plainUsd, pctText } from '../../../lib/meridianFormat';

export const PortfolioPane = ({ filter = '' }: { filter?: string }) => {
  const { summary, pools, note, loading } = usePortfolio();
  const f = filter.trim().toLowerCase();
  const rows = pools.filter((p) => !f || (p.poolName ?? '').toLowerCase().includes(f));
  const pnlPositive = Number(summary.totalPnlUsd ?? 0) >= 0;

  return (
    <section className="mrd-panel" aria-label="Historical DLMM positions">
      <span className="mrd-panel-label">HISTORICAL — DLMM POSITIONS</span>
      <span className="mrd-panel-right">{summary.closedCount ?? 0} CLOSED</span>

      <div className="mrd-pf-summary">
        <div className="cell"><span className="k">TOTAL PNL</span><span className={`v ${pnlPositive ? 'up' : 'down'}`}>{plainUsd(summary.totalPnlUsd)} <em style={{ fontSize: 11, fontStyle: 'normal' }}>{pctText(summary.totalPnlPct)}</em></span></div>
        <div className="cell"><span className="k">DEPOSIT</span><span className="v">{plainUsd(summary.allTimeDepositUsd)}</span></div>
        <div className="cell"><span className="k">FEES</span><span className="v mrd-fees">{plainUsd(summary.feesClaimedUsd)}</span></div>
        <div className="cell"><span className="k">WIN RATE</span><span className="v">{Number(summary.winRate ?? 0).toFixed(1)}%</span></div>
        <div className="cell"><span className="k">AVG INV</span><span className="v">{plainUsd(summary.avgInvestedUsd)}</span></div>
      </div>

      <div className="mrd-pf-head">
        <span>POOL</span><span className="mrd-num-r">PNL</span><span className="mrd-num-r">DEPOSIT</span><span className="mrd-num-r">WITHDRAW</span><span className="mrd-num-r">FEES EARNED</span>
      </div>
      <div>
        {rows.length
          ? rows.map((p) => <PortfolioRowView key={p.pool ?? p.poolName} pool={p} />)
          : <PaneState loading={loading} message={f ? `No pools match "${filter}".` : note} rows={5} widths={['30%', '14%', '16%', '16%', '14%']} />}
      </div>
    </section>
  );
};
