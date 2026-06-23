'use client';

import { useEffect, useState } from 'react';
import {
  LayoutDashboard,
  Layers,
  Activity,
  Wallet,
  Radar,
  Settings,
  BarChart3,
  CircleDollarSign,
  Target,
  type LucideIcon,
} from 'lucide-react';
import { PositionTable } from '../trading/PositionTable';
import { PortfolioWidget } from '../trading/PortfolioWidget';
import { WeatherWidget } from '../widgets/WeatherWidget';
import { MusicWidget } from '../widgets/MusicWidget';
import { ActivityWidget } from '../widgets/ActivityWidget';
import { CandidateWidget } from '../widgets/CandidateWidget';
import { BackendControlsWidget, BackendStatusWidget } from '../widgets/BackendControlWidgets';
import { cachedJson } from '../../lib/clientCache';

type ViewId = 'overview' | 'positions' | 'activity' | 'portfolio' | 'candidates' | 'settings';

const NAV: Array<{ id: ViewId; label: string; icon: LucideIcon }> = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard },
  { id: 'positions', label: 'Positions', icon: Layers },
  { id: 'activity', label: 'Activity Log', icon: Activity },
  { id: 'portfolio', label: 'Portfolio', icon: Wallet },
  { id: 'candidates', label: 'Candidates', icon: Radar },
  { id: 'settings', label: 'Settings', icon: Settings },
];

type Tone = 'up' | 'down' | 'none';
type Stat = { label: string; value: string; icon: LucideIcon; tone: Tone };
const toneClass = (tone: Tone) => (tone === 'up' ? 'profit' : tone === 'down' ? 'loss' : '');
const formatUsd = (value: number) => `${value >= 0 ? '+' : '-'}$${Math.abs(value).toFixed(2)}`;

const ProfileNav = ({ view, setView }: { view: ViewId; setView: (v: ViewId) => void }) => {
  const [stats, setStats] = useState<Stat[]>([
    { label: 'Trades', value: '0', icon: BarChart3, tone: 'none' },
    { label: 'PnL', value: '+$0.00', icon: CircleDollarSign, tone: 'up' },
    { label: 'Open Positions', value: '0', icon: Layers, tone: 'none' },
    { label: 'Win Rate', value: '-', icon: Target, tone: 'none' },
  ]);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        // Use the same authoritative source as the Historical/Portfolio card
        // (Meteora-aggregated closed positions) so the profile stats match.
        const [status, portfolio] = await Promise.all([
          cachedJson<any>('/api/meridian/status', 8_000),
          cachedJson<any>('/api/meridian/portfolio', 60_000),
        ]);
        const active = status?.data?.active_positions ?? 0;
        const s = portfolio?.data?.summary ?? {};
        const trades = Number(s.closedCount ?? 0);
        const pnl = Number(s.totalPnlUsd ?? 0);
        const winPct = s.winRate;
        if (mounted) {
          setStats([
            { label: 'Trades', value: String(trades), icon: BarChart3, tone: 'none' },
            { label: 'PnL', value: formatUsd(pnl), icon: CircleDollarSign, tone: pnl >= 0 ? 'up' : 'down' },
            { label: 'Open Positions', value: String(active), icon: Layers, tone: 'none' },
            { label: 'Win Rate', value: winPct == null ? '-' : `${Number(winPct).toFixed(0)}%`, icon: Target, tone: winPct == null ? 'none' : Number(winPct) >= 50 ? 'up' : 'down' },
          ]);
        }
      } catch { /* keep fallback */ }
    };
    load();
    const t = window.setInterval(load, 10_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);

  return (
    <aside className="dash-sidebar">
      <div className="dash-profile">
        <div className="dash-avatar"><img src="/profile-avatar.png" alt="0xRapzz" /></div>
        <h1>0xRapzz</h1>
        <p>DLMM_AGENT</p>
      </div>
      <div className="dash-stats">
        {stats.map((s) => (
          <div className="dash-stat-row" key={s.label}>
            <s.icon size={16} />
            <span>{s.label}</span>
            <strong className={toneClass(s.tone)}>{s.value}</strong>
          </div>
        ))}
      </div>
      <nav className="dash-nav">
        {NAV.map((item) => (
          <button
            type="button"
            key={item.id}
            className={view === item.id ? 'active' : ''}
            onClick={() => setView(item.id)}
          >
            <item.icon size={18} /><span>{item.label}</span>
          </button>
        ))}
      </nav>
    </aside>
  );
};

export const DashboardView = () => {
  const [view, setView] = useState<ViewId>('overview');

  return (
    <div className="dash-shell">
      <ProfileNav view={view} setView={setView} />

      <section className="dash-main">
        {view === 'overview' && (<><PositionTable /><PortfolioWidget /></>)}
        {view === 'positions' && <PositionTable />}
        {view === 'activity' && <ActivityWidget />}
        {view === 'portfolio' && <PortfolioWidget />}
        {view === 'candidates' && <CandidateWidget />}
        {view === 'settings' && (<><BackendStatusWidget /><BackendControlsWidget /></>)}
      </section>

      <aside className="dash-rail">
        <WeatherWidget />
        <MusicWidget />
        <CandidateWidget />
      </aside>
    </div>
  );
};
