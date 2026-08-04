'use client';

import { usePositions } from '../hooks';
import { PositionRowView, PaneState } from './rows';
import type { PositionRow } from '../../../lib/meridianFormat';

type Notify = (kind: string, text: string) => void;

export const PositionsPane = ({ onNotify }: { onNotify?: Notify }) => {
  const { positions, loading } = usePositions();

  const close = (row: PositionRow) =>
    onNotify?.('warn', `close ${row.pair} — position management is bot-autonomous; run: manage`);

  return (
    <section className="mrd-panel" aria-label="Open positions">
      <span className="mrd-panel-label">OPEN POSITIONS</span>
      <span className="mrd-panel-right">{positions.length} POSITIONS</span>
      <div className="mrd-pos-head mrd-pos-grid-lg" style={{ padding: '15px 16px 9px' }}>
        <span>PRICE RANGE</span><span>YOUR LIQUIDITY</span><span>CLAIMABLE FEES</span><span className="mrd-num-r">PNL</span>
      </div>
      <div>
        {positions.length
          ? positions.map((p) => <PositionRowView key={p.key} row={p} size="lg" onClose={close} />)
          : <PaneState loading={loading} message="No active backend positions." rows={2} />}
      </div>
    </section>
  );
};
