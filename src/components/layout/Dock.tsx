'use client';

import { useEffect, useState } from 'react';
import { Briefcase, Code2, Monitor, Power, SquareAsterisk, Zap, type LucideIcon } from 'lucide-react';
import { Clock } from './Clock';
import { cachedJson } from '../../lib/clientCache';

const apps: Array<[LucideIcon, string]> = [
  [Monitor, 'Terminal'],
  [Briefcase, 'Files'],
  [SquareAsterisk, 'Browser'],
  [Code2, 'Editor'],
];

type SystemStats = {
  cpu: number;
  ramUsed: string;
  ramTotal: string;
  ramPercent: number;
};

type DockProps = {
  activeApp: string;
  onOpenApp: (app: string) => void;
};

export const Dock = ({ activeApp, onOpenApp }: DockProps) => {
  const [stats, setStats] = useState<SystemStats>({ cpu: 0, ramUsed: '0.0G', ramTotal: '0.0G', ramPercent: 0 });

  useEffect(() => {
    let isMounted = true;

    const loadStats = () => {
      cachedJson<SystemStats>('/api/system', 5_000)
        .then((data: SystemStats) => {
          if (isMounted) setStats(data);
        })
        .catch(() => undefined);
    };

    loadStats();
    const timer = window.setInterval(loadStats, 8000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  return (
  <footer className="dock">
    <button type="button" className="dock-brand" onClick={() => onOpenApp('Dashboard')}><Zap size={24} fill="currentColor" />Hyprland</button>
    <div className="divider" />
    <div className="dock-apps">
      {apps.map(([Icon, label]) => (
        <button type="button" className={activeApp === label ? 'active' : ''} onClick={() => onOpenApp(label)} key={label}><Icon size={16} />{label}</button>
      ))}
    </div>
    <div className="system-tray">
      <span>CPU <strong>{stats.cpu}%</strong></span>
      <span className="wave" />
      <span>RAM <strong>{stats.ramUsed} / {stats.ramTotal}</strong></span>
      <span className="tiny-bar" />
      <div className="dock-clock"><Clock type="time" /><Clock type="numericDate" /></div>
      <button type="button" className="power-button" aria-label="Power"><Power size={17} /></button>
    </div>
  </footer>
  );
};
