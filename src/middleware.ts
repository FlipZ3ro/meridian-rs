import { NextRequest, NextResponse } from 'next/server';
import { COOKIE_NAME, authBypassed, verifySession } from './lib/auth';

// Protect the data/control API routes: every request must carry a valid session
// cookie (issued by SIWS login). This is the real gate — without it the backend
// proxy (incl. /api/meridian/control which executes trades) would be open even
// though the page shows a lock screen. Auth routes are intentionally excluded.
export async function middleware(request: NextRequest) {
  // Local dev (or an explicit MERIDIAN_NO_AUTH=1) skips the gate so the
  // dashboard opens straight into the terminal. See authBypassed().
  if (authBypassed(request.headers.get('host'))) return NextResponse.next();

  const session = await verifySession(request.cookies.get(COOKIE_NAME)?.value);
  if (session) return NextResponse.next();
  return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
}

export const config = {
  matcher: [
    '/api/meridian/:path*',
    '/api/agent/:path*',
    '/api/wallet/:path*',
    '/api/chart/:path*',
    '/api/prices',
    '/api/system',
    '/api/weather',
  ],
};
