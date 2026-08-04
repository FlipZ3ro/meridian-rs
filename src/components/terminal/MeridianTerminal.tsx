'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { StatusLine } from './StatusLine';
import { useAgentControl, useQuickFlip, useStatus, useWallet, usePortfolio, usePositions } from './hooks';
import { OverviewPane } from './panes/OverviewPane';
import { PositionsPane } from './panes/PositionsPane';
import { PortfolioPane } from './panes/PortfolioPane';
import { RadarPane } from './panes/RadarPane';
import { LogPane } from './panes/LogPane';
import { ChartsPane } from './panes/ChartsPane';
import { SettingsPane } from './panes/SettingsPane';
import { plainUsd, shortAddr } from '../../lib/meridianFormat';

type ScreenId = 'overview' | 'positions' | 'portfolio' | 'candidates' | 'log' | 'charts' | 'settings';
type NavMode = 'tabs' | 'rail' | 'zen';
type Line = { kind: string; text: string };

const SCREENS: Array<[ScreenId, string]> = [
  ['overview', 'OVERVIEW'],
  ['positions', 'POSITIONS'],
  ['portfolio', 'PORTFOLIO'],
  ['candidates', 'RADAR'],
  ['log', 'LOG'],
  ['charts', 'CHARTS'],
  ['settings', 'SETTINGS'],
];

const NAV_MODES: NavMode[] = ['tabs', 'rail', 'zen'];

const GOTO: Record<string, ScreenId> = {
  overview: 'overview', ov: 'overview', dash: 'overview',
  positions: 'positions', pos: 'positions', open: 'positions',
  portfolio: 'portfolio', closed: 'portfolio', history: 'portfolio',
  candidates: 'candidates', radar: 'candidates', screening: 'candidates',
  log: 'log', decisions: 'log', activity: 'log',
  charts: 'charts', bb: 'charts', chart: 'charts',
  settings: 'settings', cfg: 'settings', config: 'settings',
};

const HELP = [
  'COMMANDS',
  '  overview | positions | portfolio      switch pane',
  '  radar | log | charts | settings       switch pane',
  '  status            agent + backend snapshot',
  '  balance           wallet balance',
  '  pnl               performance summary',
  '  filter <text>     filter the current table',
  '  nav tabs|rail|zen navigation style',
  '  start|stop|restart   trading agent process',
  '  arm|disarm        quick-flip scalper',
  '  clear             clear this console',
  'KEYS  1–7 panes · : focus prompt · / filter · esc close',
].join('\n');

const LINE_COLOR: Record<string, string> = {
  cmd: 'var(--bright)',
  out: 'var(--soft)',
  err: 'var(--red)',
  info: 'var(--purple)',
  warn: 'var(--amber)',
};

const KEY_HINTS = [
  ['1–7', 'PANES'], [':', 'PROMPT'], ['/', 'FILTER'],
  ['↑↓', 'HISTORY'], ['?', 'HELP'], ['ESC', 'CLOSE'],
];

const pad = (value: string, n: number) => (value + '                    ').slice(0, n);

export default function MeridianTerminal() {
  const [screen, setScreen] = useState<ScreenId>('overview');
  const [nav, setNav] = useState<NavMode>('tabs');
  const [cmd, setCmd] = useState('');
  const [lines, setLines] = useState<Line[]>([
    { kind: 'info', text: "meridian terminal ready — type 'help', or press 1-7 to switch panes" },
  ]);
  const [consoleOpen, setConsoleOpen] = useState(false);
  const [history, setHistory] = useState<string[]>([]);
  const [hIdx, setHIdx] = useState<number | null>(null);
  const [filter, setFilter] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  const status = useStatus();
  const agent = useAgentControl();
  const flip = useQuickFlip();
  const sol = useWallet();
  const positions = usePositions();
  const { summary } = usePortfolio();

  const push = useCallback((kind: string, text: string) => {
    setLines((prev) => [...prev, { kind, text }].slice(-160));
    setConsoleOpen(true);
  }, []);

  const agentOnline = agent.online;
  const agentLabel = agentOnline ? 'RUNNING' : agent.status === 'stopped' ? 'STOPPED' : agent.status.toUpperCase();

  const run = useCallback(async (raw: string) => {
    const command = raw.trim();
    if (!command) return;
    push('cmd', `❯ ${command}`);
    setHistory((prev) => [...prev, command]);
    setHIdx(null);

    const parts = command.split(/\s+/);
    const verb = parts[0].toLowerCase();
    const arg = (parts[1] ?? '').toLowerCase();

    if (GOTO[verb]) {
      setScreen(GOTO[verb]);
      push('out', `pane → ${GOTO[verb]}`);
      return;
    }
    if (verb === 'help' || verb === '?') { push('info', HELP); return; }
    if (verb === 'clear') { setLines([]); setConsoleOpen(false); return; }
    if (verb === 'nav') {
      if (NAV_MODES.includes(arg as NavMode)) { setNav(arg as NavMode); push('out', `nav → ${arg}`); }
      else push('err', 'usage: nav tabs|rail|zen');
      return;
    }
    if (verb === 'filter') {
      const text = command.slice(command.indexOf(' ') + 1).trim();
      if (!parts[1]) { setFilter(''); push('out', 'filter cleared'); return; }
      setFilter(text);
      push('out', `filter → "${text}"  (radar / log / portfolio)`);
      return;
    }
    if (verb === 'status') {
      push('out', [
        pad('status', 20) + (agentOnline ? 'running' : 'stopped'),
        pad('mode', 20) + (status?.dry_run ? 'DRY RUN' : 'LIVE'),
        pad('wallet', 20) + shortAddr(status?.wallet),
        pad('active positions', 20) + String(status?.active_positions ?? positions.length),
        pad('screen every', 20) + `${status?.schedule?.screeningIntervalMin ?? '-'} min`,
        pad('manage every', 20) + `${status?.schedule?.managementIntervalMin ?? '-'} min`,
        pad('pnl poll', 20) + `${status?.schedule?.pnlPollIntervalSecs ?? '-'} sec`,
        pad('state', 20) + (status?.state_path ? 'connected' : 'not set'),
        pad('data dir', 20) + (status?.data_dir ? 'available' : 'unknown'),
        pad('quick-flip', 20) + (flip.armed ? 'ARMED' : 'off'),
      ].join('\n'));
      return;
    }
    if (verb === 'balance') {
      push('out', `◎ ${sol == null ? '…' : sol.toFixed(3)} SOL   ·  wallet ${shortAddr(status?.wallet)}`);
      return;
    }
    if (verb === 'pnl' || verb === 'performance') {
      push('out', [
        pad('trades', 18) + String(summary.closedCount ?? 0),
        pad('win rate', 18) + `${Number(summary.winRate ?? 0).toFixed(1)}%`,
        pad('total pnl', 18) + `${plainUsd(summary.totalPnlUsd)}  (${Number(summary.totalPnlPct ?? 0).toFixed(2)}%)`,
        pad('deposit', 18) + plainUsd(summary.allTimeDepositUsd),
        pad('fees claimed', 18) + plainUsd(summary.feesClaimedUsd),
        pad('avg invested', 18) + plainUsd(summary.avgInvestedUsd),
      ].join('\n'));
      return;
    }
    if (verb === 'start' || verb === 'stop' || verb === 'restart') {
      if (verb === 'start' && agentOnline) { push('err', 'agent already running'); return; }
      if (verb === 'stop' && !agentOnline) { push('err', 'agent already stopped'); return; }
      if (verb === 'stop' && !window.confirm('Stop the trading agent? It will stop screening and managing positions until you start it again.')) {
        push('out', 'stop cancelled');
        return;
      }
      const res = await agent.act(verb);
      push(res.ok ? 'out' : 'err', res.ok ? `meridian-backend → ${res.status ?? verb}` : 'agent control failed');
      return;
    }
    if (verb === 'arm' || verb === 'disarm') {
      const next = verb === 'arm';
      if (next && flip.live) {
        const size = flip.state?.params?.deploy_amount_sol ?? 0.2;
        if (!window.confirm(`Arm quick-flip in LIVE mode? The bot will deploy REAL positions (~${size} SOL each) on qualifying volume spikes.`)) {
          push('out', 'arm cancelled');
          return;
        }
      }
      const res = await flip.setEnabled(next);
      if (!res.ok) { push('err', 'quick-flip request failed'); return; }
      push(next ? 'warn' : 'out', next
        ? `quick-flip ARMED · ${flip.live ? 'LIVE' : 'dry run'} — deploys ◎${flip.state?.params?.deploy_amount_sol ?? 0.2} per qualifying volume spike`
        : 'quick-flip disarmed');
      return;
    }
    push('err', `unknown command: ${verb}. Type 'help'.`);
  }, [agent, agentOnline, flip, positions.length, push, sol, status, summary]);

  // Global keyboard: 1–7 panes, ':' prompt, '/' filter, '?' help, esc close.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName ?? '';
      const typing = tag === 'INPUT' || tag === 'TEXTAREA';
      if (!typing && e.key >= '1' && e.key <= '7') {
        setScreen(SCREENS[Number(e.key) - 1][0]);
        return;
      }
      if (!typing && (e.key === ':' || e.key === '/')) {
        e.preventDefault();
        setCmd(e.key === '/' ? 'filter ' : '');
        inputRef.current?.focus();
        return;
      }
      if (!typing && e.key === '?') {
        e.preventDefault();
        push('info', HELP);
        return;
      }
      if (e.key === 'Escape') {
        setConsoleOpen(false);
        inputRef.current?.blur();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [push]);

  const onCmdKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      const value = cmd;
      setCmd('');
      void run(value);
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (!history.length) return;
      const next = hIdx === null ? history.length - 1 : Math.max(0, hIdx - 1);
      setHIdx(next);
      setCmd(history[next]);
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (hIdx === null) return;
      const next = hIdx + 1;
      if (next >= history.length) { setHIdx(null); setCmd(''); }
      else { setHIdx(next); setCmd(history[next]); }
    }
  };

  const screenLabel = useMemo(() => (SCREENS.find((s) => s[0] === screen) ?? ['', ''])[1], [screen]);

  const vitals = [
    { label: 'TRADES', value: String(summary.closedCount ?? 0), color: 'var(--bright)' },
    { label: 'PNL', value: plainUsd(summary.totalPnlUsd), color: Number(summary.totalPnlUsd ?? 0) >= 0 ? 'var(--green)' : 'var(--red)' },
    { label: 'OPEN', value: String(status?.active_positions ?? positions.length), color: 'var(--bright)' },
    { label: 'WIN RATE', value: `${Number(summary.winRate ?? 0).toFixed(0)}%`, color: 'var(--green)' },
  ];

  const navButtons = (small = false) => (
    <>
      <span>NAV</span>
      {NAV_MODES.map((m) => (
        <button
          type="button"
          key={m}
          className={`mrd-navmode-btn ${nav === m ? 'active' : ''}`}
          style={small ? { fontSize: 9 } : undefined}
          onClick={() => setNav(m)}
        >
          {m.toUpperCase()}
        </button>
      ))}
    </>
  );

  const walletText = sol == null ? '…' : sol.toFixed(3);
  const notify = (kind: string, text: string) => push(kind, text);

  const pane = (() => {
    switch (screen) {
      case 'positions': return <PositionsPane onNotify={notify} />;
      case 'portfolio': return <PortfolioPane filter={filter} />;
      case 'candidates': return <RadarPane filter={filter} />;
      case 'log': return <LogPane filter={filter} />;
      case 'charts': return <ChartsPane />;
      case 'settings': return <SettingsPane onNotify={notify} />;
      default: return <OverviewPane filter={filter} />;
    }
  })();

  return (
    <div className="mrd">
      <StatusLine agentOnline={agentOnline} agentLabel={agentLabel} status={status} />

      {nav === 'tabs' ? (
        <div className="mrd-tabs">
          {SCREENS.map(([id, label], i) => (
            <button type="button" key={id} className={`mrd-tab ${screen === id ? 'active' : ''}`} onClick={() => setScreen(id)}>
              <span className="num">{i + 1}</span>
              <span className="label">{label}</span>
              {screen === id ? <span className="underline" /> : null}
            </button>
          ))}
          <div className="mrd-tabs-fill" />
          <div className="mrd-navmode">{navButtons()}</div>
        </div>
      ) : null}

      <div className="mrd-body">
        {nav === 'rail' ? (
          <div className="mrd-rail">
            <div className="mrd-rail-box mrd-rail-user">
              <span className="mrd-corner">USER</span>
              <div className="name">OxRapzz</div>
              <div className="row">
                <span className="bal">◎ {walletText} SOL</span>
                <span className="badge-live">{status?.dry_run ? 'DRY' : 'LIVE'}</span>
              </div>
            </div>

            <div className="mrd-rail-box mrd-rail-vitals">
              <span className="mrd-corner">VITALS</span>
              {vitals.map((v) => (
                <div className="mrd-vital-row" key={v.label}>
                  <span className="k">{v.label}</span>
                  <span className="v" style={{ color: v.color }}>{v.value}</span>
                </div>
              ))}
            </div>

            <div className="mrd-rail-nav">
              {SCREENS.map(([id, label], i) => (
                <button
                  type="button"
                  key={id}
                  className={`mrd-rail-btn ${screen === id ? 'active' : ''}`}
                  style={{ borderLeftColor: screen === id ? 'var(--bright)' : 'var(--dim)' }}
                  onClick={() => setScreen(id)}
                >
                  <span style={{ color: screen === id ? 'var(--purple)' : 'var(--fainter)' }}>{i + 1}</span>
                  <span style={{ color: screen === id ? 'var(--bright)' : 'var(--dim)' }}>{label}</span>
                </button>
              ))}
            </div>

            <div className="mrd-rail-grow" />
            <div className="mrd-rail-foot">{navButtons(true)}</div>
          </div>
        ) : null}

        <div className="mrd-main">
          {nav === 'zen' ? (
            <div className="mrd-zen">
              <span className="caret">❯</span>
              <span className="screen">{screenLabel}</span>
              <span className="mrd-sep">│</span>
              <span className="hint">press 1–7 or type a command · ? for help</span>
              <div className="mrd-spacer" />
              {navButtons(true)}
            </div>
          ) : null}

          <div className="mrd-scroll">
            {nav !== 'rail' ? (
              <div className="mrd-vitalstrip">
                <div className="cell" style={{ minWidth: 160 }}>
                  <span className="k">USER</span>
                  <span className="v" style={{ color: 'var(--bright)', letterSpacing: '.1em' }}>OxRapzz</span>
                </div>
                <div className="cell" style={{ minWidth: 150 }}>
                  <span className="k">WALLET</span>
                  <span className="v" style={{ color: 'var(--green)' }}>◎ {walletText} SOL</span>
                </div>
                {vitals.map((v) => (
                  <div className="cell" key={v.label}>
                    <span className="k">{v.label}</span>
                    <span className="v" style={{ color: v.color }}>{v.value}</span>
                  </div>
                ))}
                <div className="flags">
                  <span className="flag live">{status?.dry_run ? 'DRY RUN' : 'LIVE'}</span>
                  <span className="flag open">{positions.length} OPEN</span>
                </div>
              </div>
            ) : null}

            {filter.trim() ? (
              <div className="mrd-filterbar">
                <span className="tag">FILTER</span>
                <span className="val">{filter}</span>
                <span className="note">· applies to radar / log / portfolio</span>
                <div className="mrd-spacer" />
                <button type="button" className="clear" onClick={() => setFilter('')}>CLEAR</button>
              </div>
            ) : null}

            {pane}
          </div>

          {consoleOpen && lines.length ? (
            <div className="mrd-console">
              {lines.map((line, i) => (
                <div className="mrd-console-line" key={i} style={{ color: LINE_COLOR[line.kind] ?? 'var(--soft)' }}>{line.text}</div>
              ))}
            </div>
          ) : null}

          <div className="mrd-cmdbar">
            <span className="host">meridian</span>
            <span className="tilde">~</span>
            <span className="caret">❯</span>
            <input
              ref={inputRef}
              type="text"
              value={cmd}
              spellCheck={false}
              autoComplete="off"
              placeholder="type a command — help, positions, radar, arm, nav rail…"
              onChange={(e) => setCmd(e.target.value)}
              onKeyDown={onCmdKey}
            />
            {consoleOpen ? <button type="button" className="esc" onClick={() => setConsoleOpen(false)}>ESC</button> : null}
          </div>

          <div className="mrd-hints">
            {KEY_HINTS.map(([key, label]) => (
              <span className="mrd-hint" key={key}>
                <span className="key">{key}</span>
                <span>{label}</span>
              </span>
            ))}
            <div className="grow" />
            <span className="mrd-session">session meridian · window {screenLabel.toLowerCase()} · pane 0</span>
          </div>
        </div>
      </div>
    </div>
  );
}
