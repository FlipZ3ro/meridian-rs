import { NextResponse } from 'next/server';

export const runtime = 'nodejs';
// Always re-execute — never serve a cached balance (was the main reason the
// widget looked frozen).
export const dynamic = 'force-dynamic';
export const revalidate = 0;

// Public key of the bot wallet (a PUBLIC key — safe to read/show). Falls back to
// the known address if the env isn't set.
const PUBKEY =
  process.env.MERIDIAN_WALLET_PUBKEY ||
  process.env.MERIDIAN_WALLET ||
  '7kXcXZsXzeL1dPsAY4LyVbWFwgrvZVUM6HuuMwCvjjwf';

// getBalance is light; public RPC is fine. Override with SOLANA_RPC_URL (e.g.
// Helius) if rate limits bite.
const RPC = process.env.SOLANA_RPC_URL || 'https://api.mainnet-beta.solana.com';

// GET /api/wallet/balance — live SOL balance of the bot wallet. Middleware-gated.
export async function GET() {
  try {
    const res = await fetch(RPC, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'getBalance', params: [PUBKEY] }),
      cache: 'no-store',
    });
    const json = await res.json();
    const lamports = Number(json?.result?.value ?? 0);
    return NextResponse.json({ ok: true, pubkey: PUBKEY, sol: lamports / 1e9 });
  } catch {
    return NextResponse.json({ ok: false, error: 'rpc error' }, { status: 502 });
  }
}
