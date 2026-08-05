import { NextRequest, NextResponse } from 'next/server';
import { exec } from 'node:child_process';
import { promisify } from 'node:util';

// Runs on the VPS Node runtime (needs child_process) — not Edge.
export const runtime = 'nodejs';

const execRaw = promisify(exec);
// Ensure pm2 is found even if the server process has a minimal PATH.
const execAsync = (cmd: string) =>
  execRaw(cmd, { env: { ...process.env, PATH: `${process.env.PATH ?? ''}:/usr/bin:/usr/local/bin` } });

// The pm2 process name of the trading bot. Frontend + tunnel are left running;
// only this backend agent is started/stopped.
const AGENT = 'meridian-backend';

// Fixed command map — NO user input is ever interpolated into the shell, so the
// action selector can't be abused for command injection. This route is admin-
// gated by middleware (a valid session cookie is required to reach it).
const COMMANDS: Record<string, string> = {
  start: `pm2 start ${AGENT}`,
  stop: `pm2 stop ${AGENT}`,
  restart: `pm2 restart ${AGENT}`,
};

const agentStatus = async (): Promise<string> => {
  try {
    const { stdout } = await execAsync('pm2 jlist');
    const list = JSON.parse(stdout) as Array<{ name?: string; pm2_env?: { status?: string } }>;
    const proc = list.find((p) => p.name === AGENT);
    return proc?.pm2_env?.status ?? 'unknown';
  } catch {
    return 'unknown';
  }
};

// GET /api/agent/control — current agent status (online | stopped | …).
export async function GET() {
  return NextResponse.json({ ok: true, agent: AGENT, status: await agentStatus() });
}

// POST /api/agent/control { action: 'start' | 'stop' | 'restart' }
export async function POST(request: NextRequest) {
  let body: { action?: string };
  try {
    body = await request.json();
  } catch {
    return NextResponse.json({ ok: false, error: 'invalid body' }, { status: 400 });
  }
  const action = String(body.action ?? '');
  const cmd = COMMANDS[action];
  if (!cmd) {
    return NextResponse.json({ ok: false, error: `unknown action '${action}'` }, { status: 400 });
  }
  try {
    await execAsync(cmd);
    return NextResponse.json({ ok: true, action, status: await agentStatus() });
  } catch (error) {
    console.error(`[agent control] ${action} failed:`, error);
    return NextResponse.json({ ok: false, error: 'agent control failed' }, { status: 500 });
  }
}
