import { NextRequest, NextResponse } from 'next/server';
import { checkLogin, COOKIE_NAME, passwordLoginEnabled, signSession } from '../../../../lib/auth';

// In-memory brute-force throttle, keyed by client IP. Single long-running Next
// server on the VPS, so module state persists. 5 attempts / 5 min, then lock.
type Attempt = { count: number; until: number };
const attempts: Map<string, Attempt> = (globalThis as any).__meridianLoginAttempts ?? new Map();
(globalThis as any).__meridianLoginAttempts = attempts;

const WINDOW_MS = 5 * 60 * 1000;
const MAX_ATTEMPTS = 5;

const clientIp = (req: NextRequest): string =>
  req.headers.get('cf-connecting-ip') ||
  req.headers.get('x-forwarded-for')?.split(',')[0]?.trim() ||
  'unknown';

// POST /api/auth/login { password } — gate session issuance behind a shared
// password. Works on any browser (mobile included) where wallet injection isn't
// available. Issues the same HMAC session cookie as the SIWS flow.
export async function POST(request: NextRequest) {
  if (!passwordLoginEnabled()) {
    return NextResponse.json({ error: 'password login disabled' }, { status: 403 });
  }

  const ip = clientIp(request);
  const now = Date.now();
  const rec = attempts.get(ip);
  if (rec && rec.count >= MAX_ATTEMPTS && rec.until > now) {
    const secs = Math.ceil((rec.until - now) / 1000);
    return NextResponse.json({ error: `too many attempts — wait ${secs}s` }, { status: 429 });
  }

  let body: { username?: string; password?: string };
  try {
    body = await request.json();
  } catch {
    return NextResponse.json({ error: 'invalid body' }, { status: 400 });
  }
  const username = (body.username || '').trim();
  const password = (body.password || '').trim();
  if (!password) {
    return NextResponse.json({ error: 'username and password required' }, { status: 400 });
  }

  if (!checkLogin(username, password)) {
    const next = !rec || rec.until <= now ? { count: 1, until: now + WINDOW_MS } : { count: rec.count + 1, until: rec.until };
    attempts.set(ip, next);
    return NextResponse.json({ error: 'wrong username or password' }, { status: 401 });
  }

  attempts.delete(ip); // success clears the throttle
  const token = await signSession(username || 'admin');
  const res = NextResponse.json({ ok: true });
  res.cookies.set(COOKIE_NAME, token, {
    httpOnly: true,
    sameSite: 'lax',
    secure: process.env.NODE_ENV === 'production',
    path: '/',
    maxAge: 60 * 60 * 12,
  });
  return res;
}
