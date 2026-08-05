import { NextRequest, NextResponse } from 'next/server';

// Agent Meridian chart-indicators endpoint returns raw 5m OHLCV candles.
// Built-in public key (base64 "meridian-is-the-best-agents") — same one the
// backend uses; PUBLIC_API_KEY env overrides it if set.
// `||` (not `??`) so an empty-string env var still falls back to the default.
const AGENT_MERIDIAN_BASE =
  (process.env.AGENT_MERIDIAN_API_URL || '').trim() || 'https://api.agentmeridian.xyz/api';
const PUBLIC_API_KEY =
  (process.env.PUBLIC_API_KEY || '').trim() || 'bWVyaWRpYW4taXMtdGhlLWJlc3QtYWdlbnRz';

type RouteContext = { params: Promise<{ mint: string }> };

export async function GET(request: NextRequest, context: RouteContext) {
  const { mint } = await context.params;
  const interval = request.nextUrl.searchParams.get('interval') ?? '5_MINUTE';
  const candles = request.nextUrl.searchParams.get('candles') ?? '120';

  if (!mint) {
    return NextResponse.json({ error: 'mint required' }, { status: 400 });
  }

  const url = `${AGENT_MERIDIAN_BASE.replace(/\/$/, '')}/chart-indicators/${mint}?interval=${encodeURIComponent(interval)}&candles=${encodeURIComponent(candles)}&rsiLength=2`;

  try {
    const response = await fetch(url, {
      cache: 'no-store',
      headers: { 'x-api-key': PUBLIC_API_KEY, accept: 'application/json' },
    });
    const payload = await response.json().catch(() => null);
    if (!response.ok || !payload) {
      return NextResponse.json(
        { error: payload?.error ?? `chart-indicators ${response.status}`, candles: [] },
        { status: response.ok ? 502 : response.status },
      );
    }
    const list = Array.isArray(payload?.candles) ? payload.candles : [];
    return NextResponse.json({ mint, interval, candles: list });
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'chart fetch failed', candles: [] },
      { status: 502 },
    );
  }
}
