'use client';

import { useEffect, useState } from 'react';
import { ChartNoAxesColumnIncreasing } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type Decision = {
  timestamp?: string;
  tool?: string;
  action?: string;
  pair?: string;
  pool?: string;
  args?: {
    pool?: string;
    pool_address?: string;
    position_id?: string;
  };
  message?: string;
  reason?: string;
  resultSummary?: string;
  result?: string | {
    note?: string;
    pool?: string;
    success?: boolean;
  };
  success?: boolean;
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

const shortPool = (value?: string) => value ? `${value.slice(0, 4)}...${value.slice(-4)}` : '-';

const decisionText = (decision: Decision) => {
  if (decision.message) return decision.message;
  if (decision.reason) return decision.reason;
  if (decision.resultSummary) return decision.resultSummary;
  if (typeof decision.result === 'string') return decision.result;
  if (decision.result?.note) return decision.result.note;
  return decision.tool ?? decision.action ?? 'Backend decision';
};

const mapDecision = (decision: Decision): [string, string, string, string] => {
  const pool = decision.pair ?? decision.pool ?? decision.args?.pool ?? decision.args?.pool_address ?? (typeof decision.result === 'object' ? decision.result.pool : undefined);

  return [
    formatAge(decision.timestamp),
    decision.success === false ? 'no_deploy' : 'deploy',
    shortPool(pool),
    decisionText(decision),
  ];
};

export const ActivityWidget = () => {
  const [logs, setLogs] = useState<Array<[string, string, string, string]>>([]);

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
        const fallbackLogs: Array<[string, string, string, string]> = status ? [
          ['now', 'deploy', '-', `Backend ${status.status ?? 'running'} · dryRun=${status.dry_run ? 'true' : 'false'}`],
          ['now', 'deploy', '-', `Active positions: ${status.active_positions ?? 0}`],
          ['now', 'deploy', '-', `Screen ${status.schedule?.screeningIntervalMin ?? '-'}m · Manage ${status.schedule?.managementIntervalMin ?? '-'}m`],
          ['now', 'deploy', '-', `State: ${status.state_path ? 'connected' : 'not set'}`],
        ] : [];

        if (isMounted) setLogs(decisions.length ? decisions.slice(0, 8).map(mapDecision) : fallbackLogs);
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
    <GlassCard className="activity-card terminal-activity">
      <div className="terminal-title"><ChartNoAxesColumnIncreasing size={18} />ACTIVITY LOG</div>
      <div className="terminal-divider" />
      <div className="activity-head"><span>TIME</span><span>EVENT</span><span>PAIR</span><span>MESSAGE</span></div>
      <div className="log-list">
        {logs.length ? logs.map(([time, type, pair, text], index) => (
          <div className="log-row" key={`${time}-${index}`}>
            <span>{time}</span>
            <b className={type === 'deploy' ? 'deploy' : 'no-deploy'}>{type === 'deploy' ? 'OK' : 'SKIP'}</b>
            <strong>{pair}</strong>
            <p><i>$</i> {text}</p>
          </div>
        )) : <div className="activity-empty">No backend decisions yet.</div>}
      </div>
      <div className="activity-footer"><span>/api/meridian/decisions</span><span>{logs.length} entries</span></div>
    </GlassCard>
  );
};
