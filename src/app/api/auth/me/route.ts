import { NextRequest, NextResponse } from 'next/server';
import { COOKIE_NAME, authBypassed, verifySession } from '../../../../lib/auth';

// GET /api/auth/me — report whether the current session cookie is valid.
// AuthGate calls this on mount, so reporting "authed" here is what drops the
// lock screen and boots straight into the terminal.
export async function GET(request: NextRequest) {
  if (authBypassed(request.headers.get('host'))) {
    return NextResponse.json({ authed: true, pubkey: 'local-dev', bypass: true });
  }
  const session = await verifySession(request.cookies.get(COOKIE_NAME)?.value);
  if (!session) return NextResponse.json({ authed: false }, { status: 200 });
  return NextResponse.json({ authed: true, pubkey: session.pubkey });
}
