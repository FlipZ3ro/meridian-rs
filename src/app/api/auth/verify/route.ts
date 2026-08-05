import { NextRequest, NextResponse } from 'next/server';
import nacl from 'tweetnacl';
import bs58 from 'bs58';
import { buildSignMessage, consumeNonce, COOKIE_NAME, isAllowed, signSession } from '../../../../lib/auth';

// POST /api/auth/verify { pubkey, signature } — verify the SIWS signature over
// the issued nonce message, check the allowlist, and set the session cookie.
export async function POST(request: NextRequest) {
  let body: { pubkey?: string; signature?: string };
  try {
    body = await request.json();
  } catch {
    return NextResponse.json({ error: 'invalid body' }, { status: 400 });
  }
  const pubkey = (body.pubkey || '').trim();
  const signature = (body.signature || '').trim();
  if (!pubkey || !signature) {
    return NextResponse.json({ error: 'pubkey and signature required' }, { status: 400 });
  }

  if (!isAllowed(pubkey)) {
    return NextResponse.json({ error: 'wallet not allowed' }, { status: 403 });
  }

  const nonce = consumeNonce(pubkey);
  if (!nonce) {
    return NextResponse.json({ error: 'nonce expired — retry' }, { status: 400 });
  }

  let valid = false;
  try {
    valid = nacl.sign.detached.verify(
      new TextEncoder().encode(buildSignMessage(nonce)),
      bs58.decode(signature),
      bs58.decode(pubkey),
    );
  } catch {
    valid = false;
  }
  if (!valid) {
    return NextResponse.json({ error: 'signature verification failed' }, { status: 401 });
  }

  const token = await signSession(pubkey);
  const res = NextResponse.json({ ok: true, pubkey });
  res.cookies.set(COOKIE_NAME, token, {
    httpOnly: true,
    sameSite: 'lax',
    secure: process.env.NODE_ENV === 'production',
    path: '/',
    maxAge: 60 * 60 * 12,
  });
  return res;
}
