'use client';

import { useEffect, useState } from 'react';
import { Radar } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type Candidate = {
  name?: string;
  pool_address?: string;
  score?: number;
  tvl?: number;
  volume?: number;
  fees_sol?: number;
  fee_active_tvl_ratio?: number;
  volatility?: number;
};

const formatCompact = (value?: number) => {
  if (value == null || !Number.isFinite(value)) return '-';
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toFixed(value >= 10 ? 0 : 2);
};

export const CandidateWidget = () => {
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [filteredReason, setFilteredReason] = useState('Loading candidates...');

  useEffect(() => {
    let isMounted = true;

    const loadCandidates = async () => {
      try {
        const payload = await cachedJson<any>('/api/meridian/candidates?limit=40', 60_000);
        const nextCandidates = Array.isArray(payload?.data?.candidates) ? payload.data.candidates.slice(0, 10) : [];
        const filtered = Array.isArray(payload?.data?.filtered_examples) ? payload.data.filtered_examples : [];

        if (isMounted) {
          setCandidates(nextCandidates);
          setFilteredReason(nextCandidates.length ? `${nextCandidates.length} candidates passed` : filtered[0]?.reason ?? 'No candidates passed');
        }
      } catch {
        if (isMounted) {
          setCandidates([]);
          setFilteredReason('Backend unavailable');
        }
      }
    };

    loadCandidates();
    const timer = window.setInterval(loadCandidates, 60_000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <GlassCard className="candidate-card terminal-candidates">
      <div className="terminal-title"><Radar size={18} />CANDIDATE RADAR</div>
      <div className="terminal-divider" />
      <div className="candidate-head"><span>PAIR</span><span>SCORE</span><span>TVL</span><span>FEES</span></div>
      <div className="candidate-list">
        {candidates.length ? candidates.map((candidate) => (
          <div className="candidate-row" key={candidate.pool_address ?? candidate.name}>
            <div>
              <strong>{candidate.name ?? 'UNKNOWN'}</strong>
              <small>{candidate.pool_address ? `${candidate.pool_address.slice(0, 5)}...${candidate.pool_address.slice(-5)}` : '-'}</small>
            </div>
            <span>{formatCompact(candidate.score)}</span>
            <span>${formatCompact(candidate.tvl)}</span>
            <span className="profit">{formatCompact(candidate.fees_sol)} SOL</span>
          </div>
        )) : <div className="candidate-empty">{filteredReason}</div>}
      </div>
      <div className="candidate-footer"><span>/api/meridian/candidates</span><span>{filteredReason}</span></div>
    </GlassCard>
  );
};
