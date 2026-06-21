'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { Sidebar } from './Sidebar';
import { Topbar } from './Topbar';
import { Dock } from './Dock';
import { PositionTable } from '../trading/PositionTable';
import { PortfolioWidget } from '../trading/PortfolioWidget';
import { WeatherWidget } from '../widgets/WeatherWidget';
import { MusicWidget } from '../widgets/MusicWidget';
import { ActivityWidget } from '../widgets/ActivityWidget';
import { CandidateWidget } from '../widgets/CandidateWidget';
import { BackendControlsWidget, BackendStatusWidget } from '../widgets/BackendControlWidgets';
import { TerminalConsole } from './TerminalConsole';
import { cachedJson } from '../../lib/clientCache';

type SystemInfo = {
  cpu: number;
  cpuModel: string;
  cpuSpeed: string;
  cores: number;
  platform: string;
  release: string;
  hostname: string;
  uptime: number;
  memory: string;
  ramTotal: string;
  ramPercent: number;
  gpu: string;
  disks: Array<{ id: string; used: string; total: string; percent: number }>;
};

type WidgetId = 'profile' | 'positions' | 'trades' | 'weather' | 'music' | 'activity' | 'candidates' | 'backendStatus' | 'backendControls';

type WidgetLayout = {
  workspace: number;
  x: number;
  y: number;
  width: number;
  height: number;
  minWidth: number;
  minHeight: number;
  z: number;
};

const defaultWidgets: Record<WidgetId, WidgetLayout> = {
  // Workspace 1 — three bottom-aligned columns ending at ~1036px:
  //   left rail  (profile + candidates),
  //   center     (open positions + historical — the hero),
  //   right rail (weather + now-playing + activity, widened so log text fits).
  profile: { workspace: 1, x: 0, y: 0, width: 300, height: 470, minWidth: 260, minHeight: 430, z: 1 },
  candidates: { workspace: 1, x: 0, y: 486, width: 300, height: 672, minWidth: 280, minHeight: 260, z: 1 },
  positions: { workspace: 1, x: 316, y: 0, width: 868, height: 336, minWidth: 660, minHeight: 240, z: 1 },
  trades: { workspace: 1, x: 316, y: 352, width: 868, height: 806, minWidth: 520, minHeight: 360, z: 1 },
  weather: { workspace: 1, x: 1200, y: 0, width: 496, height: 264, minWidth: 340, minHeight: 220, z: 1 },
  music: { workspace: 1, x: 1200, y: 280, width: 496, height: 296, minWidth: 360, minHeight: 250, z: 1 },
  activity: { workspace: 1, x: 1200, y: 592, width: 496, height: 566, minWidth: 360, minHeight: 280, z: 1 },
  backendStatus: { workspace: 2, x: 0, y: 0, width: 620, height: 360, minWidth: 420, minHeight: 300, z: 1 },
  backendControls: { workspace: 2, x: 636, y: 0, width: 720, height: 560, minWidth: 520, minHeight: 420, z: 1 },
};

const fallbackSystem: SystemInfo = {
  cpu: 0,
  cpuModel: 'Loading CPU',
  cpuSpeed: '0.00 GHz',
  cores: 0,
  platform: 'win32',
  release: '-',
  hostname: 'dlmm-agent',
  uptime: 0,
  memory: '0MiB / 0MiB',
  ramTotal: '0G',
  ramPercent: 0,
  gpu: 'Loading GPU',
  disks: [],
};

const formatUptime = (seconds: number) => {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${days}d ${hours}h ${minutes}m`;
};

const formatCpu = (systemInfo: SystemInfo) => {
  const model = systemInfo.cpuModel
    .replace(/Intel\(R\)\s*/gi, '')
    .replace(/Core\(TM\)\s*/gi, '')
    .replace(/CPU\s*/gi, '')
    .replace(/@\s*[\d.]+\s*GHz/gi, '')
    .replace(/\s+/g, ' ')
    .trim();

  return `${model} @ ${systemInfo.cpuSpeed.replace(' ', '')}`;
};

const formatGpu = (gpu: string) => {
  const match = gpu.match(/RTX\s.+/i);
  return match?.[0]?.replace(/\s+/g, ' ').trim() ?? gpu;
};

const toCapacity = (value: string) => {
  const amount = Number.parseFloat(value);
  if (!Number.isFinite(amount)) return value;
  if (amount >= 1024) return `${(amount / 1024).toFixed(amount >= 10240 ? 0 : 1)}TB`;
  return `${Math.round(amount)}GB`;
};

export default function DashboardLayout() {
  const [isHydrated, setIsHydrated] = useState(false);
  const [workspace, setWorkspace] = useState(1);
  const [activeApp, setActiveApp] = useState('Dashboard');
  const [terminalPosition, setTerminalPosition] = useState({ x: 14, y: 8 });
  const [terminalSize, setTerminalSize] = useState({ width: 942, height: 560 });
  const [terminalMinimized, setTerminalMinimized] = useState(false);
  const [terminalMaximized, setTerminalMaximized] = useState(false);
  const [terminalSnap, setTerminalSnap] = useState<'left' | 'right' | null>(null);
  const [widgets, setWidgets] = useState(defaultWidgets);
  const [topWidgetZ, setTopWidgetZ] = useState(5);
  const [systemInfo, setSystemInfo] = useState<SystemInfo>(fallbackSystem);

  useEffect(() => {
    setIsHydrated(true);
  }, []);

  useEffect(() => {
    let isMounted = true;

    const loadSystem = () => {
      cachedJson<SystemInfo>('/api/system', 5_000)
        .then((data: SystemInfo) => {
          if (isMounted) setSystemInfo(data);
        })
        .catch(() => undefined);
    };

    loadSystem();
    const timer = window.setInterval(loadSystem, 8000);
    return () => {
      isMounted = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (!isHydrated) return;

    const clampWidgets = () => {
      const dashboard = document.querySelector('.dashboard-grid') as HTMLDivElement | null;
      const rect = dashboard?.getBoundingClientRect();
      if (!rect) return;

      setWidgets((current) => Object.fromEntries(
        (Object.entries(current) as Array<[WidgetId, WidgetLayout]>).map(([id, layout]) => {
          const width = Math.min(layout.width, rect.width);
          const height = Math.min(layout.height, rect.height);
          return [id, {
            ...layout,
            width,
            height,
            x: Math.max(0, Math.min(layout.x, rect.width - width)),
            y: Math.max(0, Math.min(layout.y, rect.height - height)),
          }];
        }),
      ) as Record<WidgetId, WidgetLayout>);
    };

    clampWidgets();
    window.addEventListener('resize', clampWidgets);
    return () => window.removeEventListener('resize', clampWidgets);
  }, [isHydrated]);

  if (!isHydrated) {
    return <main className="desktop-shell" suppressHydrationWarning />;
  }

  const handleTerminalDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const overlay = event.currentTarget.closest('.terminal-overlay') as HTMLDivElement | null;
    const parent = overlay?.parentElement as HTMLDivElement | null;
    const overlayRect = overlay?.getBoundingClientRect();
    const parentRect = parent?.getBoundingClientRect();
    const startX = event.clientX;
    const startY = event.clientY;
    const bounds = parentRect ? { width: parentRect.width, height: parentRect.height } : { width: window.innerWidth, height: window.innerHeight };
    const pointerInParent = parentRect ? { x: startX - parentRect.left, y: startY - parentRect.top } : { x: startX, y: startY };
    const dragSize = terminalMaximized
      ? {
          width: Math.min(Math.max(terminalSize.width, 900), bounds.width - 20),
          height: Math.min(Math.max(terminalSize.height, 560), bounds.height - 20),
        }
      : overlayRect
        ? { width: overlayRect.width, height: overlayRect.height }
        : terminalSize;
    const pointerRatio = overlayRect ? Math.min(0.85, Math.max(0.15, (startX - overlayRect.left) / overlayRect.width)) : 0.5;
    const startPosition = terminalMaximized
      ? {
          x: Math.max(0, Math.min(pointerInParent.x - dragSize.width * pointerRatio, bounds.width - dragSize.width)),
          y: Math.max(0, Math.min(pointerInParent.y - 15, bounds.height - dragSize.height)),
        }
      : overlayRect && parentRect
        ? { x: overlayRect.left - parentRect.left, y: overlayRect.top - parentRect.top }
        : terminalPosition;
    let lastX = event.clientX;
    let lastY = event.clientY;
    setTerminalSnap(null);
    setTerminalMaximized(false);
    if (terminalMaximized) {
      setTerminalSize(dragSize);
      setTerminalPosition(startPosition);
    }
    event.currentTarget.setPointerCapture(event.pointerId);

    const handleMove = (moveEvent: PointerEvent) => {
      lastX = moveEvent.clientX;
      lastY = moveEvent.clientY;
      const nextX = startPosition.x + moveEvent.clientX - startX;
      const nextY = startPosition.y + moveEvent.clientY - startY;
      setTerminalPosition({
        x: Math.max(0, Math.min(nextX, bounds.width - dragSize.width)),
        y: Math.max(0, Math.min(nextY, bounds.height - dragSize.height)),
      });
    };

    const handleUp = () => {
      const snapThreshold = 24;
      const projectedX = Math.max(0, startPosition.x + lastX - startX);
      const projectedY = Math.max(0, startPosition.y + lastY - startY);
      const releasePointer = parentRect ? { x: lastX - parentRect.left, y: lastY - parentRect.top } : { x: lastX, y: lastY };

      if (releasePointer.y <= snapThreshold) {
        setTerminalMaximized(true);
        setTerminalSnap(null);
      } else if (releasePointer.x <= snapThreshold) {
        setTerminalMaximized(false);
        setTerminalSnap('left');
      } else if (releasePointer.x >= bounds.width - snapThreshold) {
        setTerminalMaximized(false);
        setTerminalSnap('right');
      } else {
        setTerminalPosition({
          x: Math.max(0, Math.min(projectedX, bounds.width - dragSize.width)),
          y: Math.max(0, Math.min(projectedY, bounds.height - dragSize.height)),
        });
      }
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  };

  const resizeTerminal = (event: React.PointerEvent<HTMLDivElement>, direction: string) => {
    if (terminalMaximized) return;

    const overlay = event.currentTarget.closest('.terminal-overlay') as HTMLDivElement | null;
    const parent = overlay?.parentElement as HTMLDivElement | null;
    const overlayRect = overlay?.getBoundingClientRect();
    const parentRect = parent?.getBoundingClientRect();

    const startX = event.clientX;
    const startY = event.clientY;
    const startPosition = overlayRect && parentRect
      ? { x: overlayRect.left - parentRect.left, y: overlayRect.top - parentRect.top }
      : terminalPosition;
    const startSize = overlayRect ? { width: overlayRect.width, height: overlayRect.height } : terminalSize;
    const bounds = parentRect ? { width: parentRect.width, height: parentRect.height } : { width: window.innerWidth, height: window.innerHeight };
    setTerminalSnap(null);
    event.currentTarget.setPointerCapture(event.pointerId);

    const handleMove = (moveEvent: PointerEvent) => {
      const dx = moveEvent.clientX - startX;
      const dy = moveEvent.clientY - startY;
      const nextSize = { ...startSize };
      const nextPosition = { ...startPosition };

      if (direction.includes('e')) nextSize.width = startSize.width + dx;
      if (direction.includes('s')) nextSize.height = startSize.height + dy;
      if (direction.includes('w')) {
        nextSize.width = startSize.width - dx;
        nextPosition.x = startPosition.x + dx;
      }
      if (direction.includes('n')) {
        nextSize.height = startSize.height - dy;
        nextPosition.y = startPosition.y + dy;
      }

      const clampedWidth = Math.max(640, Math.min(nextSize.width, bounds.width - Math.max(0, nextPosition.x)));
      const clampedHeight = Math.max(380, Math.min(nextSize.height, bounds.height - Math.max(0, nextPosition.y)));
      const clampedX = Math.max(0, Math.min(nextPosition.x, bounds.width - clampedWidth));
      const clampedY = Math.max(0, Math.min(nextPosition.y, bounds.height - clampedHeight));

      setTerminalSize({ width: clampedWidth, height: clampedHeight });
      setTerminalPosition({ x: clampedX, y: clampedY });
    };

    const handleUp = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  };

  const focusWidget = (id: WidgetId) => {
    const nextZ = topWidgetZ + 1;
    setTopWidgetZ(nextZ);
    setWidgets((current) => ({
      ...current,
      [id]: { ...current[id], z: nextZ },
    }));
  };

  const moveWidgetToWorkspace = (id: WidgetId, nextWorkspace: number) => {
    setWidgets((current) => ({
      ...current,
      [id]: { ...current[id], workspace: nextWorkspace },
    }));
    setWorkspace(nextWorkspace);
  };

  const getWorkspaceDropTarget = (x: number, y: number) => {
    const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>('[data-workspace-target]'));
    const target = tabs.find((tab) => {
      const rect = tab.getBoundingClientRect();
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    });

    return target ? Number(target.dataset.workspaceTarget) : null;
  };

  const dragWidget = (event: React.PointerEvent<HTMLDivElement>, id: WidgetId) => {
    const target = event.target as HTMLElement;
    if (target.closest('button, input, a, iframe, .resize-handle, .widget-resize-handle')) return;

    const widget = event.currentTarget.closest('.dashboard-widget') as HTMLDivElement | null;
    const parent = widget?.parentElement as HTMLDivElement | null;
    const parentRect = parent?.getBoundingClientRect();
    if (!parentRect) return;

    focusWidget(id);
    const startX = event.clientX;
    const startY = event.clientY;
    const startLayout = widgets[id];
    let lastX = event.clientX;
    let lastY = event.clientY;
    event.currentTarget.setPointerCapture(event.pointerId);

    const handleMove = (moveEvent: PointerEvent) => {
      lastX = moveEvent.clientX;
      lastY = moveEvent.clientY;
      const nextX = startLayout.x + moveEvent.clientX - startX;
      const nextY = startLayout.y + moveEvent.clientY - startY;
      setWidgets((current) => ({
        ...current,
        [id]: {
          ...current[id],
          x: Math.max(0, Math.min(nextX, parentRect.width - startLayout.width)),
          y: Math.max(0, Math.min(nextY, parentRect.height - startLayout.height)),
        },
      }));
    };

    const handleUp = () => {
      const dropWorkspace = getWorkspaceDropTarget(lastX, lastY);
      if (dropWorkspace) moveWidgetToWorkspace(id, dropWorkspace);
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  };

  const resizeWidget = (event: React.PointerEvent<HTMLDivElement>, id: WidgetId, direction: string) => {
    const widget = event.currentTarget.closest('.dashboard-widget') as HTMLDivElement | null;
    const parent = widget?.parentElement as HTMLDivElement | null;
    const parentRect = parent?.getBoundingClientRect();
    if (!parentRect) return;

    focusWidget(id);
    const startX = event.clientX;
    const startY = event.clientY;
    const startLayout = widgets[id];
    event.currentTarget.setPointerCapture(event.pointerId);

    const handleMove = (moveEvent: PointerEvent) => {
      const dx = moveEvent.clientX - startX;
      const dy = moveEvent.clientY - startY;
      const next = { ...startLayout };

      if (direction.includes('e')) next.width = startLayout.width + dx;
      if (direction.includes('s')) next.height = startLayout.height + dy;
      if (direction.includes('w')) {
        next.width = startLayout.width - dx;
        next.x = startLayout.x + dx;
      }
      if (direction.includes('n')) {
        next.height = startLayout.height - dy;
        next.y = startLayout.y + dy;
      }

      const width = Math.max(startLayout.minWidth, Math.min(next.width, parentRect.width - Math.max(0, next.x)));
      const height = Math.max(startLayout.minHeight, Math.min(next.height, parentRect.height - Math.max(0, next.y)));
      const x = Math.max(0, Math.min(next.x, parentRect.width - width));
      const y = Math.max(0, Math.min(next.y, parentRect.height - height));

      setWidgets((current) => ({
        ...current,
        [id]: { ...current[id], x, y, width, height },
      }));
    };

    const handleUp = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  };

  const dashboardWidget = (id: WidgetId, children: ReactNode) => {
    const layout = widgets[id];

    return (
      <div
        className={`dashboard-widget widget-${id}`}
        style={{ transform: `translate(${layout.x}px, ${layout.y}px)`, width: layout.width, height: layout.height, zIndex: layout.z }}
        onPointerDown={() => focusWidget(id)}
      >
        <div className="widget-drag-surface" onPointerDown={(event) => dragWidget(event, id)} />
        {children}
        {['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'].map((direction) => (
          <div className={`widget-resize-handle ${direction}`} onPointerDown={(event) => resizeWidget(event, id, direction)} key={direction} />
        ))}
      </div>
    );
  };

  const closeTerminal = () => {
    setActiveApp('Dashboard');
    setTerminalMinimized(false);
    setTerminalSnap(null);
  };

  const toggleMaximizeTerminal = () => {
    setTerminalMaximized((maximized) => !maximized);
    setTerminalSnap(null);
    setTerminalMinimized(false);
  };

  const miniTerminal = (
    <div className="mini-terminal">
      <div className="mini-terminal-bar" onPointerDown={handleTerminalDrag}>
        <div className="mac-lights">
          <button type="button" className="close" onPointerDown={(event) => event.stopPropagation()} onClick={closeTerminal} aria-label="Close terminal" />
          <button type="button" className="minimize" onPointerDown={(event) => event.stopPropagation()} onClick={() => setTerminalMinimized(true)} aria-label="Minimize terminal" />
          <button type="button" className="maximize" onPointerDown={(event) => event.stopPropagation()} onClick={toggleMaximizeTerminal} aria-label="Maximize terminal" />
        </div>
        <span>-zsh</span>
        <div className="terminal-spacer" />
      </div>
      <div className="mini-terminal-body">
        <div className="neofetch">
          <div className="neofetch-logo">
            <img src="/neofetch-logo-transparent.png" alt="Meridian line art" />
          </div>
          <div className="neofetch-info">
            <p><b>meridian@{systemInfo.hostname}</b></p>
            <div className="neofetch-rule" />
            <span><i>OS</i><em>{systemInfo.platform} {systemInfo.release}</em></span>
            <span><i>Host</i><em>{systemInfo.hostname}</em></span>
            <span><i>Uptime</i><em>{formatUptime(systemInfo.uptime)}</em></span>
            <span><i>CPU</i><em>{formatCpu(systemInfo)}</em></span>
            <span><i>GPU</i><em>{formatGpu(systemInfo.gpu)}</em></span>
            <span><i>Memory</i><em>{toCapacity(systemInfo.ramTotal)}</em></span>
            {systemInfo.disks.slice(0, 2).map((disk) => (
              <span key={disk.id}><i>Disk {disk.id}</i><em>{toCapacity(disk.total)}</em></span>
            ))}
            <div className="color-strip"><em /><em /><em /><em /><em /><em /><em /><em /></div>
          </div>
        </div>
        <TerminalConsole />
      </div>
    </div>
  );

  const openApp = (app: string) => {
    setActiveApp(app);
    if (app === 'Terminal') setTerminalMinimized(false);
    if (app === 'Dashboard' || app === 'Terminal') {
      setWorkspace(1);
      return;
    }

    setWorkspace(0);
  };

  return (
    <main className="desktop-shell">
      <div className="ambient ambient-a" />
      <div className="ambient ambient-b" />
      <Topbar activeWorkspace={workspace || 1} onWorkspaceChange={(nextWorkspace) => { setActiveApp('Dashboard'); setWorkspace(nextWorkspace); }} />
      {workspace >= 1 && workspace <= 5 ? (
        <div className="dashboard-grid">
          {widgets.profile.workspace === workspace ? dashboardWidget('profile', <Sidebar />) : null}
          {widgets.positions.workspace === workspace ? dashboardWidget('positions', <PositionTable />) : null}
          {widgets.trades.workspace === workspace ? dashboardWidget('trades', <PortfolioWidget />) : null}
          {widgets.weather.workspace === workspace ? dashboardWidget('weather', <WeatherWidget />) : null}
          {widgets.music.workspace === workspace ? dashboardWidget('music', <MusicWidget />) : null}
          {widgets.candidates.workspace === workspace ? dashboardWidget('candidates', <CandidateWidget />) : null}
          {widgets.activity.workspace === workspace ? dashboardWidget('activity', <ActivityWidget />) : null}
          {widgets.backendStatus.workspace === workspace ? dashboardWidget('backendStatus', <BackendStatusWidget />) : null}
          {widgets.backendControls.workspace === workspace ? dashboardWidget('backendControls', <BackendControlsWidget />) : null}
          {activeApp === 'Terminal' && !terminalMinimized ? (
            <div
              className={`terminal-overlay ${terminalMaximized ? 'maximized terminal-large' : terminalSnap ? `snap-${terminalSnap} terminal-wide` : terminalSize.width > 860 || terminalSize.height > 520 ? 'terminal-wide' : ''}`}
              style={terminalMaximized || terminalSnap ? undefined : { transform: `translate(${terminalPosition.x}px, ${terminalPosition.y}px)`, width: terminalSize.width, height: terminalSize.height }}
            >
              {miniTerminal}
              {['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'].map((direction) => (
                <div className={`resize-handle ${direction}`} onPointerDown={(event) => resizeTerminal(event, direction)} key={direction} />
              ))}
            </div>
          ) : null}
        </div>
      ) : (
        <div className="empty-workspace" />
      )}
      <Dock activeApp={activeApp} onOpenApp={openApp} />
    </main>
  );
}
