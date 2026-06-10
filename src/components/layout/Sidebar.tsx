'use client';

import { useEffect, useState } from 'react';
import { BarChart3, CircleDollarSign, Layers, Target, type LucideIcon } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

const formatSol = (value: number) => `${value >= 0 ? '+ ' : '- '}$${Math.abs(value).toFixed(2)}`;

export const Sidebar = () => {
  const [stats, setStats] = useState<Array<[string, string, LucideIcon]>>([
    ['Trades', '0', BarChart3],
    ['PnL', '+ $0.00', CircleDollarSign],
    ['Open Positions', '0', Layers],
    ['Win Rate', '-', Target],
  ]);

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
        const winRate = history.win_rate_pct == null ? '-' : `${Number(history.win_rate_pct).toFixed(0)}%`;

        if (isMounted) {
          setStats([
            ['Trades', String(trades), BarChart3],
            ['PnL', formatSol(pnl), CircleDollarSign],
            ['Open Positions', String(activePositions), Layers],
            ['Win Rate', winRate, Target],
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
        <div className="avatar"><img src="/profile-avatar.png" alt="0xRapzz avatar" /></div>
        <h1>0xRapzz</h1>
        <p>DLMM_AGENT</p>
      </div>

      <div className="stat-list">
        {stats.map(([label, value, Icon]) => (
          <div className="stat-row" key={label}>
            <Icon size={21} />
            <span>{label}</span>
            <strong className={label === 'PnL' ? 'profit' : ''}>{value}</strong>
          </div>
        ))}
      </div>
    </GlassCard>
  );
};
