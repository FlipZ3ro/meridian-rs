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
import { PositionsCard } from '../trading/PositionsCard';
import { WeatherWidget } from '../widgets/WeatherWidget';
import { MusicWidget } from '../widgets/MusicWidget';
import { ActivityWidget } from '../widgets/ActivityWidget';
import { CandidateWidget } from '../widgets/CandidateWidget';
import { BackendStatusWidget, AgentControlWidget, QuickFlipControlWidget } from '../widgets/BackendControlWidgets';
import { cachedJson } from '../../lib/clientCache';

type ViewId = 'overview' | 'positions' | 'activity' | 'portfolio' | 'candidates' | 'settings';

// Order mirrors the overview layout top-to-bottom (Portfolio/History →
// Positions → Candidates), then the remaining views.
const NAV: Array<{ id: ViewId; label: string; icon: LucideIcon }> = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard },
  { id: 'portfolio', label: 'Portfolio', icon: Wallet },
  { id: 'positions', label: 'Positions', icon: Layers },
  { id: 'candidates', label: 'Candidates', icon: Radar },
  { id: 'activity', label: 'Activity Log', icon: Activity },
  { id: 'settings', label: 'Settings', icon: Settings },
];

type Tone = 'up' | 'down' | 'none';
type Stat = { label: string; value: string; icon: LucideIcon; tone: Tone };
const toneClass = (tone: Tone) => (tone === 'up' ? 'profit' : tone === 'down' ? 'loss' : '');
const formatUsd = (value: number) => `${value >= 0 ? '+' : '-'}$${Math.abs(value).toFixed(2)}`;

// Live SOL balance of the bot wallet, polled from /api/wallet/balance.
const WalletBalance = () => {
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
  return (
    <div className="dash-balance">
      <span className="dash-balance-label">WALLET</span>
      <strong>◎ {sol == null ? '…' : sol.toFixed(3)}<em> SOL</em></strong>
    </div>
  );
};

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
          cachedJson<any>('/api/meridian/status', 4_000),
          cachedJson<any>('/api/meridian/portfolio', 30_000),
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
    const t = window.setInterval(load, 5_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);

  return (
    <aside className="dash-sidebar">
      <div className="dash-profile">
        <div className="dash-avatar"><img src="/profile-avatar.png" alt="OxRapzz" /></div>
        <h1>OxRapzz</h1>
        <WalletBalance />
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
    <div className="dash-shell" data-view={view}>
      <ProfileNav view={view} setView={setView} />

      <section className="dash-main">
        {view === 'overview' && (<><PositionsCard /><CandidateWidget /><ActivityWidget className="activity-mobile-only" /></>)}
        {view === 'positions' && <PositionTable />}
        {view === 'activity' && <ActivityWidget />}
        {view === 'portfolio' && <PortfolioWidget />}
        {view === 'candidates' && <CandidateWidget />}
        {view === 'settings' && (<><AgentControlWidget /><QuickFlipControlWidget /><BackendStatusWidget /></>)}
      </section>

      <aside className="dash-rail">
        <WeatherWidget />
        <MusicWidget />
        <ActivityWidget className="activity-desktop-only" />
      </aside>
    </div>
  );
};
