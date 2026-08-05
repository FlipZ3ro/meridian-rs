'use client';

import { useEffect, useState } from 'react';
import { BarChart3, CircleDollarSign, Layers, Target, type LucideIcon } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type Tone = 'up' | 'down' | 'none';
type Stat = { label: string; value: string; icon: LucideIcon; tone: Tone };

const formatSol = (value: number) => `${value >= 0 ? '+' : '-'}$${Math.abs(value).toFixed(2)}`;
const toneClass = (tone: Tone) => (tone === 'up' ? 'profit' : tone === 'down' ? 'loss' : '');

const DEFAULT_STATS: Stat[] = [
  { label: 'Trades', value: '0', icon: BarChart3, tone: 'none' },
  { label: 'PnL', value: '+$0.00', icon: CircleDollarSign, tone: 'up' },
  { label: 'Open Positions', value: '0', icon: Layers, tone: 'none' },
  { label: 'Win Rate', value: '-', icon: Target, tone: 'none' },
];

export const Sidebar = () => {
  const [stats, setStats] = useState<Stat[]>(DEFAULT_STATS);

  useEffect(() => {
    let isMounted = true;

    const loadStats = async () => {
      try {
        const [status, performance] = await Promise.all([
          cachedJson<any>('/api/meridian/status', 8_000),
          cachedJson<any>('/api/meridian/performance', 30_000),
        ]);
        const activePositions = status?.data?.active_positions ?? 0;
        const history = performance?.data?.history ?? {};
        const trades = history.count ?? 0;
        const pnl = Number(history.total_pnl_sol ?? 0);
        const winPct = history.win_rate_pct;

        if (isMounted) {
          setStats([
            { label: 'Trades', value: String(trades), icon: BarChart3, tone: 'none' },
            { label: 'PnL', value: formatSol(pnl), icon: CircleDollarSign, tone: pnl >= 0 ? 'up' : 'down' },
            { label: 'Open Positions', value: String(activePositions), icon: Layers, tone: 'none' },
            {
              label: 'Win Rate',
              value: winPct == null ? '-' : `${Number(winPct).toFixed(0)}%`,
              icon: Target,
              tone: winPct == null ? 'none' : Number(winPct) >= 50 ? 'up' : 'down',
            },
          ]);
        }
      } catch {
        // Keep fallback values when backend is not running.
      }
    };

    loadStats();
    const timer = window.setInterval(loadStats, 10_000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <GlassCard className="sidebar-card terminal-sidebar">
      <div className="profile">
        <div className="avatar"><img src="/profile-avatar.png" alt="OxRapzz avatar" /></div>
        <h1>OxRapzz</h1>
      </div>

      <div className="stat-grid">
        {stats.map((stat) => (
          <div className="stat-tile" key={stat.label}>
            <div className="st-head"><stat.icon size={15} /><span>{stat.label}</span></div>
            <strong className={toneClass(stat.tone)}>{stat.value}</strong>
          </div>
        ))}
      </div>
    </GlassCard>
  );
};
