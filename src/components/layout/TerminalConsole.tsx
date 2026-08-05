'use client';

import { useEffect, useRef, useState } from 'react';

type Line = { kind: 'cmd' | 'out' | 'err' | 'info'; text: string };

type ApiPayload = { success?: boolean; data?: unknown; error?: string; [key: string]: unknown };

// Read-only GET endpoints exposed by the backend (reached via the Next proxy).
const GET_COMMANDS: Record<string, string> = {
  status: 'status',
  positions: 'positions',
  balance: 'balance',
  candidates: 'candidates',
  screening: 'screening',
  decisions: 'decisions',
  lessons: 'lessons',
  performance: 'performance',
  blacklist: 'blacklist',
  config: 'config',
};

// Cycle actions routed through POST /api/control.
const CONTROL_COMMANDS = new Set(['screen', 'manage']);

const HELP_LINES = [
  'Available commands:',
  '  status        agent + config snapshot',
  '  positions     open positions',
  '  balance       wallet balances',
  '  candidates    screening candidates',
  '  decisions     recent decision log',
  '  lessons       saved lessons',
  '  performance   performance history',
  '  blacklist     blacklisted tokens',
  '  config        live configuration',
  '  screen        run a screening cycle now',
  '  manage        run a management cycle now',
  '  clear         clear the terminal',
  '  help          show this help',
];

const pretty = (value: unknown): string => {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

export const TerminalConsole = () => {
  const [lines, setLines] = useState<Line[]>([
    { kind: 'info', text: "Meridian terminal ready. Type 'help' for commands." },
  ]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);

  const endRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: 'end' });
  }, [lines, busy]);

  const push = (line: Line) => setLines((prev) => [...prev, line]);

  const callApi = async (path: string, init?: RequestInit): Promise<ApiPayload> => {
    try {
      const response = await fetch(path, { cache: 'no-store', ...init });
      const payload = (await response.json().catch(() => ({}))) as ApiPayload;
      if (!response.ok) {
        return { success: false, error: payload?.error ?? `HTTP ${response.status}` };
      }
      return payload;
    } catch {
      return { success: false, error: 'request failed' };
    }
  };

  const runCommand = async (raw: string) => {
    const command = raw.trim();
    if (!command) return;

    push({ kind: 'cmd', text: command });
    setHistory((prev) => [...prev, command]);
    setHistoryIndex(null);

    const [name] = command.split(/\s+/);
    const verb = name.toLowerCase();

    if (verb === 'clear') {
      setLines([]);
      return;
    }
    if (verb === 'help' || verb === '?') {
      push({ kind: 'info', text: HELP_LINES.join('\n') });
      return;
    }

    setBusy(true);
    try {
      if (GET_COMMANDS[verb]) {
        const payload = await callApi(`/api/meridian/${GET_COMMANDS[verb]}`);
        if (payload.success === false) {
          push({ kind: 'err', text: payload.error ?? 'command failed' });
        } else {
          push({ kind: 'out', text: pretty(payload.data ?? payload) });
        }
      } else if (CONTROL_COMMANDS.has(verb)) {
        push({ kind: 'info', text: `running ${verb} cycle...` });
        const payload = await callApi('/api/meridian/control', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ action: verb, wallet_sol: 0 }),
        });
        if (payload.success === false) {
          push({ kind: 'err', text: payload.error ?? `${verb} failed` });
        } else {
          push({ kind: 'out', text: pretty(payload.data ?? payload) });
        }
      } else {
        push({ kind: 'err', text: `unknown command: ${verb}. Type 'help'.` });
      }
    } finally {
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      if (busy) return;
      const value = input;
      setInput('');
      void runCommand(value);
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (!history.length) return;
      const next = historyIndex === null ? history.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(next);
      setInput(history[next]);
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (historyIndex === null) return;
      const next = historyIndex + 1;
      if (next >= history.length) {
        setHistoryIndex(null);
        setInput('');
      } else {
        setHistoryIndex(next);
        setInput(history[next]);
      }
    }
  };

  return (
    <div className="terminal-console" onClick={() => inputRef.current?.focus()}>
      <div className="terminal-log">
        {lines.map((line, index) => {
          if (line.kind === 'cmd') {
            return (
              <div className="terminal-line cmd" key={index}>
                <b>meridian</b>
                <span>~</span>
                <code>{line.text}</code>
              </div>
            );
          }
          return (
            <pre className={`terminal-line ${line.kind}`} key={index}>
              {line.text}
            </pre>
          );
        })}
        {busy ? <div className="terminal-line info">…</div> : null}
      </div>
      <div className="terminal-input-row">
        <b>meridian</b>
        <span>~</span>
        <input
          ref={inputRef}
          className="terminal-input"
          value={input}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          placeholder={busy ? 'running…' : "type a command (try 'help')"}
          disabled={busy}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={onKeyDown}
        />
      </div>
      <div ref={endRef} />
    </div>
  );
};

export default TerminalConsole;
