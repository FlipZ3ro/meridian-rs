import { NextRequest, NextResponse } from 'next/server';
import { buildSignMessage, issueNonce } from '../../../../lib/auth';

// GET /api/auth/nonce?pubkey=<base58> — issue a one-time nonce + the message to
// sign. Public (no auth) — it only hands out a random challenge.
export async function GET(request: NextRequest) {
  const pubkey = (request.nextUrl.searchParams.get('pubkey') || '').trim();
  if (!pubkey || pubkey.length < 32 || pubkey.length > 50) {
    return NextResponse.json({ error: 'valid pubkey required' }, { status: 400 });
  }
  const nonce = issueNonce(pubkey);
  return NextResponse.json({ nonce, message: buildSignMessage(nonce) });
}
