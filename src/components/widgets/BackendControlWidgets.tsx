'use client';

import { useEffect, useState } from 'react';
import { Cpu, Settings2, ShieldCheck, TerminalSquare, WalletCards } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type ApiPayload<T = any> = { success?: boolean; data?: T; error?: string };

const api = async <T,>(path: string, init?: RequestInit): Promise<ApiPayload<T>> => {
  const response = await fetch(path, init);
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) return { success: false, error: payload?.error ?? response.statusText };
  return payload;
};

const compact = (value: unknown) => {
  const number = Number(value);
  if (!Number.isFinite(number)) return value == null || value === '' ? '-' : String(value);
  if (Math.abs(number) >= 1_000_000) return `${(number / 1_000_000).toFixed(2)}M`;
  if (Math.abs(number) >= 1_000) return `${(number / 1_000).toFixed(2)}K`;
  return Math.abs(number) >= 10 ? number.toFixed(2) : number.toFixed(4);
};

const short = (value?: string, size = 10) => value ? `${value.slice(0, size)}...${value.slice(-6)}` : '-';

const Field = ({ label, value }: { label: string; value: unknown }) => (
  <div className="backend-kv">
    <span>{label}</span>
    <strong title={String(value ?? '-')}>{String(value ?? '-')}</strong>
  </div>
);

export const BackendStatusWidget = () => {
  const [status, setStatus] = useState<any>();

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

  return (
    <GlassCard className="backend-card backend-status-card">
      <div className="terminal-title"><Cpu size={18} />BACKEND STATUS</div>
      <div className="terminal-divider" />
      <div className="backend-status-strip">
        <b>{status?.status ?? 'loading'}</b>
        <span>{status?.dry_run ? 'DRY RUN' : 'LIVE'}</span>
      </div>
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

export const BackendControlsWidget = () => {
  const [action, setAction] = useState('screen');
  const [pool, setPool] = useState('');
  const [positionId, setPositionId] = useState('');
  const [amount, setAmount] = useState('0.10');
  const [result, setResult] = useState('No action yet.');
  const [busy, setBusy] = useState(false);

  const run = async () => {
    setBusy(true);
    const body = action === 'screen' || action === 'manage'
      ? { action, wallet_sol: 0 }
      : { action, args: { pool, pool_address: pool, position_id: positionId, amount_sol: Number(amount || 0), dry_run: true, skip_swap: true } };
    const payload = await api('/api/meridian/control', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
    setResult(JSON.stringify(payload, null, 2));
    setBusy(false);
  };

  return (
    <GlassCard className="backend-card backend-controls-card">
      <div className="terminal-title"><TerminalSquare size={18} />MANUAL CONTROLS</div>
      <div className="terminal-divider" />
      <p className="backend-note">All actions go through <code>/api/meridian/control</code>. Dry-run guard stays active from backend config.</p>
      <div className="backend-form-grid">
        <label>Action<select value={action} onChange={(event) => setAction(event.target.value)}><option>screen</option><option>manage</option><option>deploy_position</option><option>claim_fees</option><option>close_position</option><option>swap_token</option></select></label>
        <label>Amount SOL<input value={amount} onChange={(event) => setAmount(event.target.value)} /></label>
        <label>Pool<input value={pool} onChange={(event) => setPool(event.target.value)} placeholder="pool address" /></label>
        <label>Position<input value={positionId} onChange={(event) => setPositionId(event.target.value)} placeholder="position id" /></label>
      </div>
      <button className="backend-button" type="button" disabled={busy} onClick={run}>{busy ? 'Executing...' : 'Execute Control'}</button>
      <pre className="backend-result">{result}</pre>
    </GlassCard>
  );
};

export const BackendConfigWidget = () => {
  const [path, setPath] = useState('management.deployAmountSol');
  const [value, setValue] = useState('0.1');
  const [config, setConfig] = useState<any>();
  const [result, setResult] = useState('');

  const load = async () => {
    const payload = await cachedJson<ApiPayload>('/api/meridian/config', 12_000).catch(() => undefined);
    setConfig(payload?.data);
  };

  useEffect(() => { load(); }, []);

  const patch = async () => {
    let parsed: unknown = value;
    try { parsed = JSON.parse(value); } catch {}
    const payload = await api('/api/meridian/config', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ path, value: parsed }) });
    setResult(JSON.stringify(payload, null, 2));
    await load();
  };

  return (
    <GlassCard className="backend-card backend-config-card">
      <div className="terminal-title"><Settings2 size={18} />CONFIG PATCH</div>
      <div className="terminal-divider" />
      <div className="backend-form-grid single">
        <label>Path<input value={path} onChange={(event) => setPath(event.target.value)} /></label>
        <label>Value<input value={value} onChange={(event) => setValue(event.target.value)} /></label>
      </div>
      <button className="backend-button" type="button" onClick={patch}>Save Patch</button>
      <div className="backend-grid-two compact">
        <Field label="Dry run" value={String(config?.dryRun ?? '-')} />
        <Field label="Deploy amount" value={`${config?.management?.deployAmountSol ?? '-'} SOL`} />
        <Field label="Max positions" value={config?.risk?.maxPositions ?? '-'} />
        <Field label="Min TVL" value={compact(config?.screening?.minTvl)} />
      </div>
      <pre className="backend-result small">{result || 'No patch yet.'}</pre>
    </GlassCard>
  );
};

export const BackendWalletLogsWidget = () => {
  const [wallet, setWallet] = useState('');
  const [balance, setBalance] = useState('Wallet required.');
  const [decisions, setDecisions] = useState<any[]>([]);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      const payload = await cachedJson<ApiPayload>('/api/meridian/decisions', 15_000).catch(() => undefined);
      if (mounted) setDecisions(Array.isArray(payload?.data?.decisions) ? payload.data.decisions.slice(0, 8) : []);
    };
    load();
    const timer = window.setInterval(load, 15_000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  const loadBalance = async () => {
    const payload = await api(`/api/meridian/balance?wallet=${encodeURIComponent(wallet)}`);
    setBalance(JSON.stringify(payload, null, 2));
  };

  return (
    <GlassCard className="backend-card backend-wallet-card">
      <div className="terminal-title"><WalletCards size={18} />WALLET & DECISIONS</div>
      <div className="terminal-divider" />
      <div className="backend-form-grid single">
        <label>Wallet address<input value={wallet} onChange={(event) => setWallet(event.target.value)} placeholder="paste wallet address" /></label>
      </div>
      <button className="backend-button" type="button" onClick={loadBalance}>Load Balance</button>
      <pre className="backend-result small">{balance}</pre>
      <div className="backend-table">
        <div className="backend-table-head"><span>Action</span><span>Status</span><span>Time</span></div>
        {decisions.length ? decisions.map((decision, index) => (
          <div className="backend-table-row" key={`${decision.timestamp}-${index}`}>
            <span>{decision.tool ?? decision.action ?? 'decision'}</span>
            <b className={decision.success === false ? 'bad' : 'ok'}>{decision.success === false ? 'failed' : 'ok'}</b>
            <span>{short(decision.timestamp, 16)}</span>
          </div>
        )) : <div className="backend-empty">No decision-log entries yet.</div>}
      </div>
    </GlassCard>
  );
};

export const BackendLessonsWidget = () => {
  const [lessons, setLessons] = useState<any[]>([]);
  const [performance, setPerformance] = useState<any>();
  const [blacklist, setBlacklist] = useState<any>();

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      const [lessonPayload, performancePayload, blacklistPayload] = await Promise.all([
        cachedJson<ApiPayload>('/api/meridian/lessons', 30_000).catch(() => undefined),
        cachedJson<ApiPayload>('/api/meridian/performance', 30_000).catch(() => undefined),
        cachedJson<ApiPayload>('/api/meridian/blacklist', 30_000).catch(() => undefined),
      ]);
      if (!mounted) return;
      setLessons(Array.isArray(lessonPayload?.data?.lessons) ? lessonPayload.data.lessons.slice(0, 5) : []);
      setPerformance(performancePayload?.data?.history);
      setBlacklist(blacklistPayload?.data);
    };
    load();
    const timer = window.setInterval(load, 30_000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  return (
    <GlassCard className="backend-card backend-lessons-card">
      <div className="terminal-title"><ShieldCheck size={18} />LESSONS / PERFORMANCE / BLACKLIST</div>
      <div className="terminal-divider" />
      <div className="backend-grid-two compact">
        <Field label="24h records" value={performance?.count ?? 0} />
        <Field label="Total PnL" value={performance?.total_pnl_sol ?? 0} />
        <Field label="Win rate" value={performance?.win_rate_pct ?? '-'} />
        <Field label="Blocks" value={`${blacklist?.tokens?.blacklist?.length ?? 0} tokens / ${blacklist?.blocked_devs?.blocked_devs?.length ?? 0} devs`} />
      </div>
      <div className="backend-table lessons">
        {lessons.length ? lessons.map((lesson, index) => (
          <div className="backend-table-row" key={index}>
            <span>{lesson.role ?? 'lesson'}</span>
            <b>{Number(lesson.confidence ?? 0).toFixed(2)}</b>
            <span>{lesson.content ?? lesson.text ?? '-'}</span>
          </div>
        )) : <div className="backend-empty">No lessons recorded yet.</div>}
      </div>
    </GlassCard>
  );
};

export const BackendApiReferenceWidget = () => (
  <GlassCard className="backend-card backend-api-card">
    <div className="terminal-title"><TerminalSquare size={18} />BACKEND API MAP</div>
    <div className="terminal-divider" />
    <p className="backend-note">Frontend dashboard now owns the control UI. Rust backend on port 3001 is treated as API service through the Next proxy.</p>
    <div className="backend-table api-map">
      {[
        ['/api/meridian/status', 'Runtime, dry-run mode, schedule, state path'],
        ['/api/meridian/positions', 'Tracked DLMM positions and simulated dry-run PnL'],
        ['/api/meridian/candidates', 'Screened pools and deploy candidates'],
        ['/api/meridian/control', 'Screen/manage/manual actions'],
        ['/api/meridian/config', 'Config summary and safe patch endpoint'],
        ['/api/meridian/decisions', 'Recent executor decisions'],
        ['/api/meridian/lessons', 'Learning records'],
        ['/api/meridian/performance', 'Performance history'],
        ['/api/meridian/blacklist', 'Token and developer blocks'],
      ].map(([path, description]) => (
        <div className="backend-table-row" key={path}>
          <span>{path}</span>
          <span>{description}</span>
        </div>
      ))}
    </div>
  </GlassCard>
);
