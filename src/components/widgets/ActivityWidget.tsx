'use client';

import { useEffect, useState } from 'react';
import { ChartNoAxesColumnIncreasing } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type Decision = {
  timestamp?: string;
  tool?: string;
  action?: string;
  type?: string;
  pair?: string;
  pool?: string;
  pool_name?: string;
  position?: string;
  args?: {
    pool?: string;
    pool_address?: string;
    position_id?: string;
  };
  message?: string;
  reason?: string;
  summary?: string | Record<string, unknown>;
  resultSummary?: string;
  result?: string | Record<string, unknown>;
  success?: boolean;
};

type LogEntry = { time: string; label: string; kind: string; pair: string; message: string };

const EVENT_COLORS: Record<string, string> = {
  deploy: '#22c55e',
  close: '#f97316',
  claim: '#2dd4bf',
  screen: '#8b5cf6',
  swap: '#38bdf8',
  fail: '#ef4444',
  skip: '#c79a4e',
  info: '#7c84a3',
};

type StatusPayload = {
  status?: string;
  dry_run?: boolean;
  active_positions?: number;
  state_path?: string;
  data_dir?: string;
  schedule?: {
    managementIntervalMin?: number;
    screeningIntervalMin?: number;
  };
};

const formatAge = (timestamp?: string) => {
  if (!timestamp) return '-';
  const time = new Date(timestamp).getTime();
  if (!Number.isFinite(time)) return '-';
  const minutes = Math.floor(Math.max(0, Date.now() - time) / 60000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
};

const shortAddr = (value?: string) => value ? `${value.slice(0, 4)}…${value.slice(-4)}` : '-';

const asObject = (value: unknown): Record<string, any> | null => {
  if (!value) return null;
  if (typeof value === 'object') return value as Record<string, any>;
  if (typeof value === 'string') {
    try { return JSON.parse(value); } catch { return null; }
  }
  return null;
};

const num = (value: unknown, digits = 4) => {
  const n = Number(value);
  return Number.isFinite(n) ? n.toFixed(digits) : null;
};

// The human reason a decision failed/was skipped (summary/result hold it).
const failReason = (decision: Decision): string => {
  const r =
    decision.resultSummary
    ?? (typeof decision.result === 'string' ? decision.result : '')
    ?? (typeof decision.summary === 'string' ? decision.summary : '')
    ?? decision.reason
    ?? '';
  return String(r);
};

// A "skip" is a deliberate gate (BB %B, dedup, cooldown, low balance) — not a
// real on-chain error. Distinguish those from true failures.
const isSkip = (reason: string): boolean => {
  const r = reason.toLowerCase();
  return r.includes('safety check')
    || r.includes('%b')
    || r.includes('already have position')
    || r.includes('waiting for')
    || r.includes('over-extended')
    || r.includes('cooldown')
    || r.includes('not enough sol')
    || r.includes('not supported')
    || r.includes('skipped')
    || r.includes('skipping')
    || r.includes('decelerat')
    || r.includes('position_id required');
};

// Classify a decision into a short event label + colour kind.
const eventOf = (decision: Decision): { label: string; kind: string } => {
  const tool = (decision.tool ?? decision.action ?? '').toLowerCase();
  if (decision.success === false) {
    return isSkip(failReason(decision)) ? { label: 'SKIP', kind: 'skip' } : { label: 'FAIL', kind: 'fail' };
  }
  if (tool.includes('deploy')) return { label: 'DEPLOY', kind: 'deploy' };
  if (tool.includes('close')) return { label: 'CLOSE', kind: 'close' };
  if (tool.includes('claim')) return { label: 'CLAIM', kind: 'claim' };
  if (tool.includes('swap')) return { label: 'SWAP', kind: 'swap' };
  if (tool.includes('screen')) return { label: 'SCREEN', kind: 'screen' };
  if (tool.includes('balance') || tool.includes('wallet')) return { label: 'INFO', kind: 'info' };
  return { label: 'OK', kind: 'info' };
};

// Build a concise, human-readable message instead of dumping raw JSON.
const humanMessage = (decision: Decision): string => {
  // For skips/failures, show the actual reason (not "Deployed position").
  if (decision.success === false) {
    const reason = failReason(decision)
      .replace(/^safety check failed:\s*/i, '')
      .replace(/\s*—\s*price not over-extended.*$/i, '')
      .replace(/,?\s*waiting for mean-reversion setup\.?$/i, '')
      .trim();
    if (reason) return reason;
  }
  const tool = (decision.tool ?? decision.action ?? '').toLowerCase();
  const data = asObject(decision.result) ?? asObject(decision.resultSummary) ?? asObject(decision.summary) ?? {};
  const name = decision.pool_name ?? (data.poolName as string) ?? '';

  if (tool.includes('balance') || tool.includes('wallet')) {
    const sol = num(data.sol ?? data.balanceSol);
    return sol ? `Wallet balance ${sol} SOL` : 'Checked wallet balance';
  }
  if (tool.includes('deploy')) {
    const amt = num(data.amountY ?? data.amount_sol, 3);
    return `Deployed ${name || 'position'}${amt ? ` · ${amt} SOL` : ''}`;
  }
  if (tool.includes('close')) {
    return `Closed ${name || 'position'}`;
  }
  if (tool.includes('claim')) {
    const fees = num(data.fees_claimed ?? data.claimable_fee_sol);
    return fees ? `Claimed ${fees} SOL fees${name ? ` · ${name}` : ''}` : `Claimed fees${name ? ` · ${name}` : ''}`;
  }
  if (tool.includes('swap')) {
    return `Swapped${name ? ` · ${name}` : ''} to SOL`;
  }
  if (tool.includes('screen')) {
    return decision.reason || (data.note as string) || 'Screening cycle';
  }
  if (decision.reason) return decision.reason;
  if (decision.message) return decision.message;
  // Default: prettify the tool name (get_token_narrative -> "Read token narrative").
  if (!tool) return 'Backend action';
  const readVerb = /^(get|list|fetch|read)_/.test(tool);
  const label = tool.replace(/^(get|list|fetch|read)_/, '').replace(/_/g, ' ');
  const text = readVerb ? `Read ${label}` : label;
  return name ? `${text} · ${name}` : text;
};

const mapDecision = (decision: Decision): LogEntry => {
  const { label, kind } = eventOf(decision);
  const pair = decision.pool_name
    ?? decision.pair
    ?? shortAddr(decision.pool ?? decision.args?.pool ?? decision.args?.pool_address ?? decision.position);
  return {
    time: formatAge(decision.timestamp),
    label,
    kind,
    pair,
    message: humanMessage(decision),
  };
};

export const ActivityWidget = ({ className = '' }: { className?: string } = {}) => {
  const [logs, setLogs] = useState<LogEntry[]>([]);

  useEffect(() => {
    let isMounted = true;

    const loadLogs = async () => {
      try {
        const [payload, statusPayload] = await Promise.all([
          cachedJson<any>('/api/meridian/decisions', 15_000),
          cachedJson<any>('/api/meridian/status', 8_000),
        ]);
        const decisions = Array.isArray(payload?.data?.decisions) ? payload.data.decisions : [];
        const status = statusPayload?.data as StatusPayload | undefined;
        const fallbackLogs: LogEntry[] = status ? [
          { time: 'now', label: 'INFO', kind: 'info', pair: '-', message: `Backend ${status.status ?? 'running'} · dryRun=${status.dry_run ? 'true' : 'false'}` },
          { time: 'now', label: 'INFO', kind: 'info', pair: '-', message: `Active positions: ${status.active_positions ?? 0}` },
          { time: 'now', label: 'INFO', kind: 'info', pair: '-', message: `Screen ${status.schedule?.screeningIntervalMin ?? '-'}m · Manage ${status.schedule?.managementIntervalMin ?? '-'}m` },
        ] : [];

        if (isMounted) setLogs(decisions.length ? decisions.slice(0, 30).map(mapDecision) : fallbackLogs);
      } catch {
        if (isMounted) setLogs([]);
      }
    };

    loadLogs();
    const timer = window.setInterval(loadLogs, 15_000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <GlassCard className={`activity-card terminal-activity ${className}`.trim()}>
      <div className="terminal-title"><ChartNoAxesColumnIncreasing size={18} />ACTIVITY LOG</div>
      <div className="terminal-divider" />
      <div className="activity-head"><span>TIME</span><span>EVENT</span><span>PAIR</span><span>MESSAGE</span></div>
      <div className="log-list">
        {logs.length ? logs.map((log, index) => (
          <div className="log-row" key={`${log.time}-${index}`}>
            <span>{log.time}</span>
            <b
              style={{
                color: EVENT_COLORS[log.kind] ?? EVENT_COLORS.info,
                background: `${EVENT_COLORS[log.kind] ?? EVENT_COLORS.info}1f`,
                border: `1px solid ${EVENT_COLORS[log.kind] ?? EVENT_COLORS.info}55`,
              }}
            >
              {log.label}
            </b>
            <strong title={log.pair}>{log.pair}</strong>
            <p title={log.message}>{log.message}</p>
          </div>
        )) : <div className="activity-empty">No backend decisions yet.</div>}
      </div>
    </GlassCard>
  );
};
