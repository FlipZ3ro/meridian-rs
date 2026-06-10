'use client';

import { useEffect, useState } from 'react';
import { Archive, Repeat2 } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type RecentEvent = {
  timestamp?: string;
  event_type?: 'Deploy' | 'Close' | 'Claim' | string;
  pool_address?: string;
  details?: string;
};

const formatTime = (timestamp?: string) => {
  if (!timestamp) return '-';
  const time = new Date(timestamp).getTime();
  if (!Number.isFinite(time)) return '-';
  const diff = Math.max(0, Date.now() - time);
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
};

const short = (value?: string) => {
  if (!value) return '-';
  if (value.length <= 14) return value;
  return `${value.slice(0, 6)}...${value.slice(-5)}`;
};

const extractPool = (details?: string) => {
  if (!details) return 'Unknown pool';
  const deployed = details.match(/pool\s+(.+)$/i)?.[1];
  const closed = details.match(/Closed\s+(.+?)\s+[—-]/i)?.[1];
  return (closed ?? deployed ?? details).trim();
};

const extractPnl = (details?: string) => {
  const value = details?.match(/PnL:\s*([-\d.]+)\s*SOL/i)?.[1];
  if (!value) return null;
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return null;
  return `${numeric >= 0 ? '+' : ''}${numeric.toFixed(4)} SOL`;
};

const eventClass = (eventType?: string) => {
  if (eventType === 'Close') return 'close';
  if (eventType === 'Claim') return 'claim';
  return 'deploy';
};

export const RecentTrades = () => {
  const [events, setEvents] = useState<RecentEvent[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;

    const load = () => {
      cachedJson<any>('/api/meridian/positions', 8_000)
        .then((payload) => {
          if (!active) return;
          const recent = Array.isArray(payload?.data?.recent_events) ? payload.data.recent_events : [];
          setEvents([...recent].reverse().slice(0, 12));
          setLoading(false);
        })
        .catch(() => {
          if (active) setLoading(false);
        });
    };

    load();
    const timer = window.setInterval(load, 10_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  const tradeEvents = events.filter((event) => event.event_type === 'Close' || event.event_type === 'Claim' || event.event_type === 'Deploy');
  const closedCount = events.filter((event) => event.event_type === 'Close').length;
  const deployCount = events.filter((event) => event.event_type === 'Deploy').length;
  const claimCount = events.filter((event) => event.event_type === 'Claim').length;

  return (
    <GlassCard className="trades-card terminal-trades">
      <div className="card-title compact">
        <div><Repeat2 size={22} /><h2>RECENT TRADES</h2></div>
        <span>{closedCount ? `${closedCount} CLOSED` : loading ? 'SYNCING' : 'EVENT LOG'}</span>
      </div>
      <div className="trade-summary-strip">
        <div><span>DEPLOY</span><b>{deployCount}</b></div>
        <div><span>CLOSE</span><b>{closedCount}</b></div>
        <div><span>CLAIM</span><b>{claimCount}</b></div>
      </div>
      {tradeEvents.length ? (
        <div className="recent-trade-list">
          {tradeEvents.map((event, index) => (
            <div className={`recent-trade-item ${eventClass(event.event_type)}`} key={`${event.timestamp}-${event.pool_address}-${index}`}>
              <div className="trade-ledger-line" />
              <div className="trade-event-main">
                <div className="trade-event-top">
                  <span className="trade-badge">{event.event_type ?? 'Event'}</span>
                  <time>{formatTime(event.timestamp)}</time>
                </div>
                <strong>{extractPool(event.details)}</strong>
                <div className="trade-event-bottom">
                  <code>{short(event.pool_address)}</code>
                  {extractPnl(event.details) ? <em>{extractPnl(event.details)}</em> : <em>{event.event_type === 'Deploy' ? 'tracking opened' : 'state event'}</em>}
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="empty-state">
          <Archive size={58} />
          <p>{loading ? '$ loading backend events...' : '$ no backend trade events yet'}</p>
        </div>
      )}
    </GlassCard>
  );
};
