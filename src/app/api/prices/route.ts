import { NextRequest, NextResponse } from 'next/server';

const SOL_MINT = 'So11111111111111111111111111111111111111112';

const numberFrom = (value: unknown) => {
  const next = Number(value);
  return Number.isFinite(next) && next > 0 ? next : null;
};

export async function GET(request: NextRequest) {
  const mints = [...new Set((request.nextUrl.searchParams.get('mints') ?? '')
    .split(',')
    .map((mint) => mint.trim())
    .filter(Boolean))];

  const [coingeckoResult, jupiterResult] = await Promise.allSettled([
    fetch('https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd', {
      cache: 'no-store',
      headers: { accept: 'application/json' },
    }),
    mints.length
      ? fetch(`https://lite-api.jup.ag/price/v3?ids=${encodeURIComponent(mints.join(','))}`, {
          cache: 'no-store',
          headers: { accept: 'application/json' },
        })
      : Promise.resolve(null),
  ]);

  let solUsd = 0;
  if (coingeckoResult.status === 'fulfilled') {
    const payload = await coingeckoResult.value.json().catch(() => null);
    solUsd = numberFrom(payload?.solana?.usd) ?? 0;
  }

  const tokenPrices: Record<string, number> = {};
  if (jupiterResult.status === 'fulfilled' && jupiterResult.value) {
    const payload = await jupiterResult.value.json().catch(() => null);
    const data = payload?.data ?? payload ?? {};
    for (const mint of mints) {
      const entry = data[mint];
      const price = numberFrom(entry?.usdPrice ?? entry?.price ?? entry);
      if (price) tokenPrices[mint] = price;
    }
  }

  if (solUsd) tokenPrices[SOL_MINT] = solUsd;

  return NextResponse.json({ solUsd, tokenPrices });
}
