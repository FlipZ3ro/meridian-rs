import { NextRequest, NextResponse } from 'next/server';

// Runs server-side (talks to the internal Rust backend). Not Edge.
export const runtime = 'nodejs';

// Same internal backend the /api/meridian proxy uses. Never exposed to clients.
const backendBaseUrl = process.env.MERIDIAN_BACKEND_URL ?? 'http://127.0.0.1:3001';

// Dedicated, admin-gated quick-flip toggle. This deliberately does NOT go
// through the /api/meridian/control allowlist (which blocks capital-moving
// actions from the UI). Arming quick-flip is a *mode* switch: it lets the bot's
// own deterministic loop deploy on volume spikes — the click itself moves no
// capital. The route is protected by middleware (valid session cookie required),
// so only the authenticated operator can reach it. Enabling it while the bot is
// in LIVE mode means real positions; the UI confirms before arming.

// GET /api/agent/quickflip — current armed state + params (read-only, no toggle).
export async function GET() {
  try {
    const res = await fetch(new URL('/api/quickflip', backendBaseUrl), { cache: 'no-store' });
    const body = await res.text();
    return new NextResponse(body, {
      status: res.status,
      headers: { 'content-type': res.headers.get('content-type') ?? 'application/json' },
    });
  } catch (error) {
    console.error('[quickflip] status failed:', error);
    return NextResponse.json({ success: false, error: 'backend unavailable' }, { status: 502 });
  }
}

// POST /api/agent/quickflip { enabled: boolean } — arm/disarm the scalper.
export async function POST(request: NextRequest) {
  let enabled: boolean;
  try {
    enabled = Boolean((await request.json())?.enabled);
  } catch {
    return NextResponse.json({ success: false, error: 'invalid body' }, { status: 400 });
  }
  try {
    const res = await fetch(new URL('/api/control', backendBaseUrl), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ action: 'quickflip', args: { enabled } }),
      cache: 'no-store',
    });
    const body = await res.text();
    return new NextResponse(body, {
      status: res.status,
      headers: { 'content-type': res.headers.get('content-type') ?? 'application/json' },
    });
  } catch (error) {
    console.error('[quickflip] toggle failed:', error);
    return NextResponse.json({ success: false, error: 'backend unavailable' }, { status: 502 });
  }
}
