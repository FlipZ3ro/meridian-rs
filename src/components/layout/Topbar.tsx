import { Music, Power, Volume2, Wifi } from 'lucide-react';
import { Clock } from './Clock';

type TopbarProps = {
  activeWorkspace: number;
  onWorkspaceChange: (workspace: number) => void;
};

const logout = async () => {
  try {
    await fetch('/api/auth/logout', { method: 'POST' });
  } catch {
    /* ignore — reload re-checks the session and falls back to the lock screen */
  }
  window.location.reload();
};

export const Topbar = ({ activeWorkspace, onWorkspaceChange }: TopbarProps) => (
  <header className="topbar">
    <div className="top-left">
      <div className="brand-mark">A</div>
      <nav className="workspace-tabs" aria-label="Workspaces">
        {[1, 2].map((item) => (
          <button type="button" data-workspace-target={item} className={item === activeWorkspace ? 'active' : ''} onClick={() => onWorkspaceChange(item)} key={item}>{item}</button>
        ))}
      </nav>
    </div>
    <div className="top-title">Meridian - DLMM Agent</div>
    <div className="top-status">
      <Music size={18} className="purple" />
      <span className="track-inline">Weird Genius & Winky Wiryawan - HEAL (feat. Venes)</span>
      <span className="online-dot" />
      <Volume2 size={16} />
      <Wifi size={16} />
      <Clock type="timeWithPeriod" />
      <button type="button" className="topbar-logout" aria-label="Log out" title="Log out" onClick={logout}><Power size={15} /></button>
    </div>
  </header>
);
