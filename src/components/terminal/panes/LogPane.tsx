'use client';

import { useDecisions } from '../hooks';
import { LogRowFullView } from './rows';

export const LogPane = ({ filter = '' }: { filter?: string }) => {
  const logs = useDecisions();
  const f = filter.trim().toLowerCase();
  const rows = logs.filter((l) => !f || `${l.badge} ${l.pair} ${l.message}`.toLowerCase().includes(f));

  return (
    <div className="mrd-panel">
      <span className="mrd-panel-label">ACTIVITY LOG</span>
      <span className="mrd-panel-right">{rows.length} EVENTS</span>
      <div className="mrd-log-full-head">
        <span>TIME</span><span>EVENT</span><span>PAIR</span><span>MESSAGE</span>
      </div>
      <div>
        {rows.length
          ? rows.map((l, i) => <LogRowFullView key={`${l.time}-${i}`} log={l} />)
          : <div className="mrd-empty">No backend decisions yet.</div>}
      </div>
    </div>
  );
};
