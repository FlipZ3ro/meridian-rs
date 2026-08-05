import { NextRequest, NextResponse } from 'next/server';
import { COOKIE_NAME, verifySession } from '../../../../lib/auth';

// GET /api/auth/me — report whether the current session cookie is valid.
export async function GET(request: NextRequest) {
  const session = await verifySession(request.cookies.get(COOKIE_NAME)?.value);
  if (!session) return NextResponse.json({ authed: false }, { status: 200 });
  return NextResponse.json({ authed: true, pubkey: session.pubkey });
}
