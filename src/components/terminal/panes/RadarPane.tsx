'use client';

import { useCandidates } from '../hooks';
import { CandidateRowView, PaneState } from './rows';

const barFor = (score: number | undefined, max: number) =>
  Math.max(4, Math.round(((Number(score ?? 0)) / (max || 1)) * 100));

export const RadarPane = ({ filter = '' }: { filter?: string }) => {
  const { candidates, note, loading } = useCandidates(40);
  const f = filter.trim().toLowerCase();
  const rows = candidates.filter((c) => !f || `${c.name ?? ''} ${c.pool_address ?? ''}`.toLowerCase().includes(f));
  const maxScore = Math.max(...candidates.map((c) => Number(c.score ?? 0)), 1);

  return (
    <section className="mrd-panel" aria-label="Candidate radar">
      <span className="mrd-panel-label">CANDIDATE RADAR</span>
      <span className="mrd-panel-right">SCREEN EVERY 1 MIN</span>
      <div className="mrd-cand-head mrd-cand-grid-lg" style={{ padding: '16px 16px 9px' }}>
        <span>PAIR</span><span>SCORE</span><span className="mrd-num-r">TVL</span><span className="mrd-num-r">FEES</span>
      </div>
      <div>
        {rows.length
          ? rows.map((c) => (
            <CandidateRowView key={c.pool_address ?? c.name} candidate={c} barPct={barFor(c.score, maxScore)} size="lg" />
          ))
          : <PaneState loading={loading} message={f ? `No candidates match "${filter}".` : note} rows={4} />}
      </div>
    </section>
  );
};
