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
  base?: { mint?: string; symbol?: string; icon?: string };
  smart_money_count?: number;
};

const formatCompact = (value?: number) => {
  if (value == null || !Number.isFinite(value)) return '-';
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toFixed(value >= 10 ? 0 : 2);
};

// Route token images through the weserv proxy/cache so slow gateways (ipfs.io)
// still load as small circular icons.
const proxiedIcon = (url?: string | null) =>
  url ? `https://wsrv.nl/?url=${encodeURIComponent(url)}&w=32&h=32&fit=cover&output=webp` : null;

// DexScreener resolves a token image directly from its mint — a reliable
// fallback when the candidate payload has no icon URL of its own.
const tokenIconUrl = (mint?: string | null) =>
  mint ? `https://dd.dexscreener.com/ds-data/tokens/solana/${mint}.png?size=lg` : null;

// Ordered logo sources: provided icon first, then DexScreener-by-mint as a
// fallback. TokenLogo advances through them on load error, then to a letter.
const logoSrcs = (icon?: string | null, mint?: string | null) =>
  [proxiedIcon(icon && icon.trim()), proxiedIcon(tokenIconUrl(mint))].filter(Boolean) as string[];

// A short label for the lettered fallback avatar.
const letterFor = (symbol?: string | null, name?: string | null) =>
  ((symbol && symbol.trim()) || (name ?? '').split(/[-/ ]/)[0] || '?').slice(0, 1).toUpperCase();

const TokenLogo = ({ srcs, symbol, name }: { srcs: string[]; symbol?: string | null; name?: string | null }) => {
  const [idx, setIdx] = useState(0);
  const src = srcs[idx];
  if (!src) {
    // Lettered avatar instead of a blank gradient so the token is still
    // identifiable when no image resolves.
    return (
      <i
        style={{
          width: 16, height: 16, borderRadius: '50%', flexShrink: 0,
          background: 'linear-gradient(135deg,#2dd4bf,#8b5cf6)',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 8, fontWeight: 800, color: '#06121b', lineHeight: 1,
        }}
      >
        {letterFor(symbol, name)}
      </i>
    );
  }
  return (
    <img
      src={src}
      alt={symbol ?? name ?? ''}
      width={16}
      height={16}
      loading="lazy"
      onError={() => setIdx((i) => i + 1)}
      style={{ width: 16, height: 16, borderRadius: '50%', objectFit: 'cover', flexShrink: 0 }}
    />
  );
};

export const CandidateWidget = ({ className = '' }: { className?: string } = {}) => {
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
    <GlassCard className={`candidate-card terminal-candidates ${className}`.trim()}>
      <div className="terminal-title"><Radar size={18} />CANDIDATE RADAR</div>
      <div className="terminal-divider" />
      <div className="candidate-head"><span>PAIR</span><span>SCORE</span><span>TVL</span><span>FEES</span></div>
      <div className="candidate-list">
        {candidates.length ? candidates.map((candidate) => (
          <div className="candidate-row" key={candidate.pool_address ?? candidate.name}>
            <div className="cand-pair">
              <TokenLogo srcs={logoSrcs(candidate.base?.icon, candidate.base?.mint)} symbol={candidate.base?.symbol} name={candidate.name} />
              <div className="cand-pair-text">
                <strong>{candidate.name ?? 'UNKNOWN'}{candidate.smart_money_count ? ` 🧠${candidate.smart_money_count}` : ''}</strong>
                <small>{candidate.pool_address ? `${candidate.pool_address.slice(0, 4)}…${candidate.pool_address.slice(-4)}` : '-'}</small>
              </div>
            </div>
            <span>{formatCompact(candidate.score)}</span>
            <span>${formatCompact(candidate.tvl)}</span>
            <span className="profit">◎{formatCompact(candidate.fees_sol)}</span>
          </div>
        )) : <div className="candidate-empty">{filteredReason}</div>}
      </div>
    </GlassCard>
  );
};
