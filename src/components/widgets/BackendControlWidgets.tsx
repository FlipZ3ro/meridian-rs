'use client';

import { useEffect, useState } from 'react';
import { Cpu, Power, Play, Square, RotateCw, Zap, Wallet } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type ApiPayload<T = any> = { success?: boolean; data?: T; error?: string };

const Field = ({ label, value }: { label: string; value: unknown }) => (
  <div className="backend-kv">
    <span>{label}</span>
    <strong title={String(value ?? '-')}>{String(value ?? '-')}</strong>
  </div>
);

const shortAddr = (a?: string) => (a && a.length > 12 ? `${a.slice(0, 4)}…${a.slice(-4)}` : a ?? '');

export const BackendStatusWidget = () => {
  const [status, setStatus] = useState<any>();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      const payload = await cachedJson<ApiPayload>('/api/meridian/status', 8_000).catch(() => undefined);
      if (mounted) setStatus(payload?.data);
    };
    load();
    const timer = window.setInterval(load, 10_000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  const wallet: string | undefined = status?.wallet;
  const copyWallet = async () => {
    if (!wallet) return;
    try {
      await navigator.clipboard.writeText(wallet);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch { /* clipboard unavailable */ }
  };

  return (
    <GlassCard className="backend-card backend-status-card">
      <div className="terminal-title"><Cpu size={18} />BACKEND STATUS</div>
      <div className="terminal-divider" />
      <div className="backend-status-strip">
        <b>{status?.status ?? 'loading'}</b>
        <span>{status?.dry_run ? 'DRY RUN' : 'LIVE'}</span>
      </div>
      {wallet && (
        <button
          type="button"
          className="backend-wallet"
          onClick={copyWallet}
          title={`${wallet}\nClick to copy`}
        >
          <Wallet size={13} />
          <code>{shortAddr(wallet)}</code>
          <span className="backend-wallet-hint">{copied ? 'copied' : 'copy'}</span>
        </button>
      )}
      <div className="backend-grid-two">
        <Field label="Active positions" value={status?.active_positions ?? 0} />
        <Field label="Screen every" value={`${status?.schedule?.screeningIntervalMin ?? '-'} min`} />
        <Field label="Manage every" value={`${status?.schedule?.managementIntervalMin ?? '-'} min`} />
        <Field label="PnL poll" value={`${status?.schedule?.pnlPollIntervalSecs ?? '-'} sec`} />
        <Field label="State" value={status?.state_path ? 'connected' : 'not set'} />
        <Field label="Data dir" value={status?.data_dir ? 'available' : 'unknown'} />
      </div>
    </GlassCard>
  );
};

// Admin-only start/stop of the trading agent (pm2 process meridian-backend).
// The frontend/dashboard and tunnel stay up regardless. Gated by middleware.
export const AgentControlWidget = () => {
  const [status, setStatus] = useState<string>('…');
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const res = await fetch('/api/agent/control', { cache: 'no-store' });
      const data = await res.json();
      setStatus(data?.status ?? 'unknown');
    } catch {
      setStatus('unknown');
    }
  };

  useEffect(() => {
    load();
    const timer = window.setInterval(load, 8_000);
    return () => window.clearInterval(timer);
  }, []);

  const act = async (action: 'start' | 'stop' | 'restart') => {
    if (busy) return;
    if (action === 'stop' && !window.confirm('Stop the trading agent? It will stop screening and managing positions until you start it again.')) return;
    setBusy(true);
    try {
      const res = await fetch('/api/agent/control', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ action }),
      });
      const data = await res.json();
      setStatus(data?.status ?? 'unknown');
    } catch {
      /* status refresh on next poll */
    } finally {
      setBusy(false);
    }
  };

  const online = status === 'online';
  const label = online ? 'RUNNING' : status === 'stopped' ? 'STOPPED' : status.toUpperCase();

  return (
    <GlassCard className="backend-card agent-control-card">
      <div className="terminal-title"><Power size={18} />AGENT CONTROL</div>
      <div className="terminal-divider" />
      <div className="agent-state">
        <span className={`agent-dot ${online ? 'on' : 'off'}`} />
        <b>{label}</b>
        <span className="agent-sub">meridian-backend</span>
      </div>
      <div className="agent-actions">
        <button type="button" className="agent-btn start" disabled={busy || online} onClick={() => act('start')}><Play size={14} />Start</button>
        <button type="button" className="agent-btn stop" disabled={busy || !online} onClick={() => act('stop')}><Square size={14} />Stop</button>
        <button type="button" className="agent-btn restart" disabled={busy} onClick={() => act('restart')}><RotateCw size={14} />Restart</button>
      </div>
      <p className="backend-note">Frontend &amp; dashboard stay online — only the trading agent starts/stops.</p>
    </GlassCard>
  );
};

// Arm/disarm the deterministic quick-flip scalper (volume-spike fast in/out).
// Independent of the LLM agent. Off by default; when armed the bot's own loop
// deploys on qualifying spikes — in LIVE mode that is real capital, so enabling
// is confirmed. Admin-gated via /api/agent/quickflip (middleware session).
type QuickFlipState = {
  enabled?: boolean;
  mode?: string;
  params?: {
    min_vol_per_min?: number;
    max_hold_min?: number;
    vol_fade_ratio?: number;
    deploy_amount_sol?: number;
  };
};

export const QuickFlipControlWidget = () => {
  const [state, setState] = useState<QuickFlipState>();
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      const res = await fetch('/api/agent/quickflip', { cache: 'no-store' });
      const data: ApiPayload<QuickFlipState> = await res.json();
      if (data?.data) setState(data.data);
    } catch {
      /* keep last known state; retry on next poll */
    }
  };

  useEffect(() => {
    load();
    const timer = window.setInterval(load, 8_000);
    return () => window.clearInterval(timer);
  }, []);

  const armed = state?.enabled === true;
  const live = state?.mode === 'live';

  const toggle = async () => {
    if (busy || state === undefined) return;
    const next = !armed;
    if (next && live) {
      const size = state?.params?.deploy_amount_sol ?? 0.2;
      if (!window.confirm(`Arm quick-flip in LIVE mode? The bot will deploy REAL positions (~${size} SOL each) on qualifying volume spikes.`)) return;
    }
    setBusy(true);
    try {
      const res = await fetch('/api/agent/quickflip', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ enabled: next }),
      });
      const data: ApiPayload<QuickFlipState> = await res.json();
      // Backend echoes the new state; fall back to optimistic value.
      setState((prev) => ({ ...prev, enabled: data?.data?.enabled ?? next, mode: data?.data?.mode ?? prev?.mode }));
    } catch {
      /* status refresh on next poll */
    } finally {
      setBusy(false);
    }
  };

  const p = state?.params;
  return (
    <GlassCard className="backend-card agent-control-card">
      <div className="terminal-title"><Zap size={18} />QUICK-FLIP SCALPER</div>
      <div className="terminal-divider" />
      <div className="agent-state">
        <span className={`agent-dot ${armed ? 'on' : 'off'}`} />
        <b>{state === undefined ? '…' : armed ? 'ARMED' : 'OFF'}</b>
        <span className="agent-sub">{state?.mode ? (live ? 'LIVE' : 'dry run') : 'volume-spike'}</span>
      </div>
      <div className="agent-actions">
        <button
          type="button"
          className={`agent-btn ${armed ? 'stop' : 'start'}`}
          disabled={busy || state === undefined}
          onClick={toggle}
        >
          {armed ? <><Square size={14} />Disarm</> : <><Play size={14} />Arm</>}
        </button>
      </div>
      <div className="backend-grid-two">
        <Field label="Entry vol/min" value={p?.min_vol_per_min ? `$${(p.min_vol_per_min / 1000).toFixed(0)}k` : '-'} />
        <Field label="Max hold" value={p?.max_hold_min ? `${p.max_hold_min} min` : '-'} />
        <Field label="Fade exit" value={p?.vol_fade_ratio ? `×${p.vol_fade_ratio}` : '-'} />
        <Field label="Size / pos" value={p?.deploy_amount_sol ? `◎${p.deploy_amount_sol}` : '-'} />
      </div>
      <p className="backend-note">Deterministic, no LLM. Enters on volume spikes, exits on fade / max-hold. Single-side SOL.</p>
    </GlassCard>
  );
};



