'use client';

import { useDecisions } from '../hooks';
import { LogRowFullView, PaneState } from './rows';

export const LogPane = ({ filter = '' }: { filter?: string }) => {
  const { logs, loading } = useDecisions();
  const f = filter.trim().toLowerCase();
  const rows = logs.filter((l) => !f || `${l.badge} ${l.pair} ${l.message}`.toLowerCase().includes(f));

  return (
    <section className="mrd-panel" aria-label="Activity log">
      <span className="mrd-panel-label">ACTIVITY LOG</span>
      <span className="mrd-panel-right">{rows.length} EVENTS</span>
      <div className="mrd-log-full-head">
        <span>TIME</span><span>EVENT</span><span>PAIR</span><span>MESSAGE</span>
      </div>
      <div>
        {rows.length
          ? rows.map((l, i) => <LogRowFullView key={`${l.time}-${i}`} log={l} />)
          : <PaneState loading={loading} message={f ? `No events match "${filter}".` : 'No backend decisions yet.'} rows={6} widths={['12%', '10%', '16%', '44%']} />}
      </div>
    </section>
  );
};
