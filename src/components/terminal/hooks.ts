'use client';

import { useEffect, useState } from 'react';
import { cachedJson } from '../../lib/clientCache';
import {
  mapPosition,
  mapDecision,
  positionMint,
  type BackendPosition,
  type Candidate,
  type PositionRow,
  type LogEntry,
  type PoolHistory,
} from '../../lib/meridianFormat';

// ── Backend status ─────────────────────────────────────────────────────
// The Next proxy answers 502 with {success:false, error:'Meridian backend
// unavailable'} when the Rust process is down. That still parses as JSON, so a
// naive reader treats it as "no data" and the panes claim the bot found
// nothing — which is a different and much more reassuring story than the truth.
export const isBackendDown = (payload: any) =>
  payload?.success === false || payload?.error === 'Meridian backend unavailable';

export type MeridianStatus = {
  status?: string;
  dry_run?: boolean;
  active_positions?: number;
  wallet?: string;
  state_path?: string;
  data_dir?: string;
  schedule?: {
    managementIntervalMin?: number;
    screeningIntervalMin?: number;
    pnlPollIntervalSecs?: number;
  };
};

export const useStatus = () => {
  const [status, setStatus] = useState<MeridianStatus>();
  const [online, setOnline] = useState<boolean | null>(null);
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      const payload = await cachedJson<any>('/api/meridian/status', 8_000).catch(() => undefined);
      if (!mounted) return;
      if (payload?.data) { setStatus(payload.data as MeridianStatus); setOnline(true); }
      else setOnline(false);
    };
    load();
    const t = window.setInterval(load, 8_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return { status, online };
};

const BACKEND_DOWN = 'Backend unreachable — meridian-backend is not responding.';

// ── Wallet balance ─────────────────────────────────────────────────────
export const useWallet = () => {
  const [sol, setSol] = useState<number | null>(null);
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const res = await fetch('/api/wallet/balance', { cache: 'no-store' });
        const data = await res.json();
        if (mounted && data?.ok) setSol(Number(data.sol));
      } catch { /* keep last known */ }
    };
    load();
    const t = window.setInterval(load, 5_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return sol;
};

// ── System (CPU / RAM) ─────────────────────────────────────────────────
export type SystemInfo = { cpu: number; ramUsed: string; ramTotal: string; memory: string; ramPercent: number };

export const useSystem = () => {
  const [info, setInfo] = useState<SystemInfo>({ cpu: 0, ramUsed: '0G', ramTotal: '0G', memory: '', ramPercent: 0 });
  useEffect(() => {
    let mounted = true;
    const load = () => {
      cachedJson<SystemInfo>('/api/system', 5_000)
        // A 401/500 still resolves with an error body — keep the last-known
        // reading unless the payload actually carries a CPU sample.
        .then((data) => { if (mounted && Number.isFinite(Number(data?.cpu))) setInfo(data); })
        .catch(() => undefined);
    };
    load();
    const t = window.setInterval(load, 8_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return info;
};

// ── Weather ────────────────────────────────────────────────────────────
export type Weather = {
  temperature: number;
  condition: string;
  location: string;
  humidity: number;
  wind: string;
  pressure: string;
  forecast: Array<{ day: string; temp: number }>;
};

const fallbackWeather: Weather = {
  temperature: 31, condition: 'Cloudy', location: 'Magelang', humidity: 86,
  wind: '2.9 km/h', pressure: '972 hPa', forecast: [],
};

export const useWeather = () => {
  const [weather, setWeather] = useState<Weather>(fallbackWeather);
  useEffect(() => {
    let mounted = true;
    fetch('/api/weather')
      .then((r) => r.json())
      .then((data: Weather) => { if (mounted && Array.isArray(data?.forecast)) setWeather(data); })
      .catch(() => undefined);
    return () => { mounted = false; };
  }, []);
  return weather;
};

// ── Open positions (mapped, priced) ────────────────────────────────────
let cachedPositionRows: PositionRow[] = [];

export const usePositions = () => {
  const [positions, setPositions] = useState<PositionRow[]>(cachedPositionRows);
  const [note, setNote] = useState('No active backend positions.');
  // "Still fetching" and "there are genuinely no positions" must not render the
  // same — an operator reading "no positions" mid-fetch would think the bot
  // exited everything.
  const [loading, setLoading] = useState(cachedPositionRows.length === 0);
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const [payload, candidatesPayload] = await Promise.all([
          cachedJson<any>('/api/meridian/positions', 4_000),
          cachedJson<any>('/api/meridian/candidates?limit=40', 60_000),
        ]);
        if (mounted) setNote(isBackendDown(payload) ? BACKEND_DOWN : 'No active backend positions.');
        const positionsPayload = Array.isArray(payload?.data?.positions)
          ? payload.data.positions as BackendPosition[]
          : [];
        const openPositions = positionsPayload.filter(
          (p) => String(p.status ?? 'active').toLowerCase() !== 'closed',
        );
        const candidates = Array.isArray(candidatesPayload?.data?.candidates)
          ? candidatesPayload.data.candidates as Candidate[]
          : [];
        const mintByPool = Object.fromEntries(candidates
          .filter((c) => c.pool_address && c.base?.mint)
          .map((c) => [c.pool_address as string, c.base?.mint as string]));
        const mints = [...new Set(openPositions
          .map((p) => positionMint(p, mintByPool))
          .filter(Boolean) as string[])];
        const prices = await cachedJson<any>(`/api/prices?mints=${encodeURIComponent(mints.join(','))}`, 30_000).catch(() => ({}));
        const pricing = {
          solUsd: Number(prices?.solUsd ?? 0),
          tokenPrices: prices?.tokenPrices ?? {},
          mintByPool,
        };
        if (Array.isArray(payload?.data?.positions)) {
          const next = openPositions.map((p) => mapPosition(p, pricing));
          cachedPositionRows = next;
          if (mounted) setPositions(next);
        } else if (mounted) {
          setPositions(cachedPositionRows);
        }
      } catch {
        if (mounted) setPositions(cachedPositionRows);
      } finally {
        if (mounted) setLoading(false);
      }
    };
    load();
    const t = window.setInterval(load, 5_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return { positions, loading, note };
};

// ── Portfolio (closed history) ─────────────────────────────────────────
export type PortfolioSummary = {
  totalPnlUsd?: number;
  totalPnlPct?: number;
  allTimeDepositUsd?: number;
  feesClaimedUsd?: number;
  closedCount?: number;
  winRate?: number;
  avgInvestedUsd?: number;
};

export type { PoolHistory } from '../../lib/meridianFormat';

export const usePortfolio = () => {
  const [summary, setSummary] = useState<PortfolioSummary>({});
  const [pools, setPools] = useState<PoolHistory[]>([]);
  const [note, setNote] = useState('No closed positions yet');
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const payload = await cachedJson<any>('/api/meridian/portfolio', 60_000);
        const nextPools = Array.isArray(payload?.data?.pools) ? payload.data.pools as PoolHistory[] : [];
        const nextSummary = (payload?.data?.summary ?? {}) as PortfolioSummary;
        if (mounted) {
          setPools(nextPools);
          setSummary(nextSummary);
          setNote(isBackendDown(payload)
            ? BACKEND_DOWN
            : nextPools.length ? `${nextSummary.closedCount ?? 0} closed positions` : 'No closed positions yet');
        }
      } catch {
        if (mounted) { setPools([]); setNote('Backend unavailable'); }
      } finally {
        if (mounted) setLoading(false);
      }
    };
    load();
    const t = window.setInterval(load, 60_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return { summary, pools, note, loading };
};

// ── Candidate radar ────────────────────────────────────────────────────
export const useCandidates = (limit = 40) => {
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [note, setNote] = useState('No candidates passed');
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const payload = await cachedJson<any>('/api/meridian/candidates?limit=40', 60_000);
        const next = Array.isArray(payload?.data?.candidates) ? (payload.data.candidates as Candidate[]).slice(0, limit) : [];
        const filtered = Array.isArray(payload?.data?.filtered_examples) ? payload.data.filtered_examples : [];
        if (mounted) {
          setCandidates(next);
          setNote(isBackendDown(payload)
            ? BACKEND_DOWN
            : next.length ? `${next.length} candidates passed` : (filtered[0]?.reason ?? 'No candidates passed'));
        }
      } catch {
        if (mounted) { setCandidates([]); setNote('Backend unavailable'); }
      } finally {
        if (mounted) setLoading(false);
      }
    };
    load();
    const t = window.setInterval(load, 60_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, [limit]);
  return { candidates, note, loading };
};

// ── Decisions (activity log) ───────────────────────────────────────────
export const useDecisions = () => {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [note, setNote] = useState('No backend decisions yet.');
  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const [payload, statusPayload] = await Promise.all([
          cachedJson<any>('/api/meridian/decisions', 15_000),
          cachedJson<any>('/api/meridian/status', 8_000),
        ]);
        const decisions = Array.isArray(payload?.data?.decisions) ? payload.data.decisions : [];
        const status = statusPayload?.data as MeridianStatus | undefined;
        const fallback: LogEntry[] = status ? [
          { time: 'now', badge: 'INFO', kind: 'info', pair: '-', message: `Backend ${status.status ?? 'running'} · dryRun=${status.dry_run ? 'true' : 'false'}` },
          { time: 'now', badge: 'INFO', kind: 'info', pair: '-', message: `Active positions: ${status.active_positions ?? 0}` },
          { time: 'now', badge: 'INFO', kind: 'info', pair: '-', message: `Screen ${status.schedule?.screeningIntervalMin ?? '-'}m · Manage ${status.schedule?.managementIntervalMin ?? '-'}m` },
        ] : [];
        if (mounted) {
          setLogs(decisions.length ? decisions.slice(0, 40).map(mapDecision) : fallback);
          setNote(isBackendDown(payload) ? BACKEND_DOWN : 'No backend decisions yet.');
        }
      } catch {
        if (mounted) setLogs([]);
      } finally {
        if (mounted) setLoading(false);
      }
    };
    load();
    const t = window.setInterval(load, 15_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);
  return { logs, loading, note };
};

// ── Agent process control ──────────────────────────────────────────────
export const useAgentControl = () => {
  const [status, setStatus] = useState<string>('…');
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const res = await fetch('/api/agent/control', { cache: 'no-store' });
      const data = await res.json();
      setStatus(data?.status ?? 'unknown');
    } catch { setStatus('unknown'); }
  };

  useEffect(() => {
    load();
    const t = window.setInterval(load, 8_000);
    return () => window.clearInterval(t);
  }, []);

  const act = async (action: 'start' | 'stop' | 'restart') => {
    if (busy) return { ok: false, message: 'busy' };
    setBusy(true);
    try {
      const res = await fetch('/api/agent/control', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ action }),
      });
      const data = await res.json();
      setStatus(data?.status ?? 'unknown');
      return { ok: true, status: data?.status };
    } catch {
      return { ok: false, message: 'request failed' };
    } finally {
      setBusy(false);
    }
  };

  const online = status === 'online';
  return { status, online, busy, act, refresh: load };
};

// ── Quick-flip scalper ─────────────────────────────────────────────────
export type QuickFlipState = {
  enabled?: boolean;
  mode?: string;
  params?: {
    min_vol_per_min?: number;
    max_hold_min?: number;
    vol_fade_ratio?: number;
    deploy_amount_sol?: number;
  };
};

export const useQuickFlip = () => {
  const [state, setState] = useState<QuickFlipState>();
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const res = await fetch('/api/agent/quickflip', { cache: 'no-store' });
      const data = await res.json();
      if (data?.data) setState(data.data as QuickFlipState);
    } catch { /* keep last known */ }
  };

  useEffect(() => {
    load();
    const t = window.setInterval(load, 8_000);
    return () => window.clearInterval(t);
  }, []);

  const setEnabled = async (next: boolean) => {
    if (busy) return { ok: false };
    setBusy(true);
    try {
      const res = await fetch('/api/agent/quickflip', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ enabled: next }),
      });
      const data = await res.json();
      setState((prev) => ({ ...prev, enabled: data?.data?.enabled ?? next, mode: data?.data?.mode ?? prev?.mode }));
      return { ok: true };
    } catch {
      return { ok: false };
    } finally {
      setBusy(false);
    }
  };

  const armed = state?.enabled === true;
  const live = state?.mode === 'live';
  return { state, armed, live, busy, setEnabled, refresh: load };
};
