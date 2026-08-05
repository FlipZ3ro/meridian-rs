'use client';

import { useEffect, useMemo, useState } from 'react';
import { CandlestickChart, Radar } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { cachedJson } from '../../lib/clientCache';

type Candle = { time: number; open: number; high: number; low: number; close: number; volume: number };

export type ChartSlot = {
  mint: string;
  name: string;
  source: 'position' | 'candidate';
};

const BB_PERIOD = 20;
const BB_MIN = 0.8; // mirror chartIndicators.bbPercentBMin

type Bands = { mid: number; upper: number; lower: number } | null;

const bollinger = (closes: number[], i: number, period = BB_PERIOD): Bands => {
  if (i < period - 1) return null;
  const window = closes.slice(i - period + 1, i + 1);
  const mid = window.reduce((a, b) => a + b, 0) / period;
  const variance = window.reduce((a, b) => a + (b - mid) ** 2, 0) / period;
  const sd = Math.sqrt(variance);
  return { mid, upper: mid + 2 * sd, lower: mid - 2 * sd };
};

const fmtPrice = (n: number) => {
  if (!Number.isFinite(n) || n <= 0) return '—';
  if (n >= 1) return n.toFixed(4);
  // subscript-zero compact notation for tiny memecoin prices
  const s = n.toFixed(12);
  const m = s.match(/^0\.(0+)(\d{1,4})/);
  if (m) return `0.0${m[1].length > 1 ? `(${m[1].length})` : ''}${m[2]}`;
  return n.toPrecision(3);
};

// Module-level candle cache keyed by mint — survives remounts (tab/widget
// switches) so a chart renders its last data instantly instead of flashing
// "Loading…" while the refetch runs.
const candleCache = new Map<string, Candle[]>();

export const ChartWidget = ({ slot, index }: { slot: ChartSlot | null; index: number }) => {
  const [candles, setCandles] = useState<Candle[]>(() =>
    slot?.mint ? candleCache.get(slot.mint) ?? [] : [],
  );
  const [status, setStatus] = useState<'idle' | 'loading' | 'ok' | 'empty' | 'error'>(() => {
    if (!slot?.mint) return 'empty';
    return (candleCache.get(slot.mint)?.length ?? 0) >= 2 ? 'ok' : 'idle';
  });

  useEffect(() => {
    if (!slot?.mint) {
      setCandles([]);
      setStatus('empty');
      return;
    }
    let mounted = true;
    // Show this mint's cached candles immediately (covers mint changes too).
    const cached = candleCache.get(slot.mint);
    if (cached?.length) {
      setCandles(cached);
      setStatus('ok');
    } else {
      setCandles([]);
    }
    const load = async () => {
      setStatus((s) => (s === 'ok' ? 'ok' : 'loading'));
      try {
        const payload = await cachedJson<{ candles: Candle[] }>(
          `/api/chart/${slot.mint}?interval=5_MINUTE&candles=120`,
          20_000,
        );
        const list = (payload?.candles ?? []).filter((c) => c && c.close > 0);
        if (!mounted) return;
        if (list.length) {
          candleCache.set(slot.mint!, list);
          setCandles(list);
          setStatus('ok');
        } else {
          // No candles returned — keep last-known if we have any.
          setStatus(candleCache.get(slot.mint!)?.length ? 'ok' : 'error');
        }
      } catch {
        // Transient error — keep showing cached candles.
        if (mounted) setStatus(candleCache.get(slot.mint!)?.length ? 'ok' : 'error');
      }
    };
    load();
    const t = window.setInterval(load, 20_000);
    return () => {
      mounted = false;
      window.clearInterval(t);
    };
  }, [slot?.mint]);

  const view = useMemo(() => {
    const last = candles.slice(-56);
    if (last.length < 2) return null;
    const closes = candles.map((c) => c.close);
    const offset = candles.length - last.length;
    const bands = last.map((_, i) => bollinger(closes, offset + i));

    let lo = Infinity;
    let hi = -Infinity;
    for (let i = 0; i < last.length; i += 1) {
      lo = Math.min(lo, last[i].low);
      hi = Math.max(hi, last[i].high);
      const b = bands[i];
      if (b) {
        lo = Math.min(lo, b.lower);
        hi = Math.max(hi, b.upper);
      }
    }
    if (!Number.isFinite(lo) || !Number.isFinite(hi) || hi <= lo) return null;

    const W = 440;
    const H = 200;
    const padX = 4;
    const padRight = 42; // right price axis
    const padTop = 8;
    const volH = 28; // volume sub-panel
    const volGap = 6;
    const priceBottom = H - volH - volGap;
    const plotW = W - padX - padRight;
    const span = hi - lo || 1;
    const x = (i: number) => padX + (i * plotW) / (last.length - 1);
    const y = (p: number) => padTop + (1 - (p - lo) / span) * (priceBottom - padTop);
    const cw = Math.max(1.4, plotW / last.length - 1.2);

    const line = (key: 'upper' | 'mid' | 'lower') =>
      bands
        .map((b, i) => (b ? `${x(i).toFixed(1)},${y(b[key]).toFixed(1)}` : null))
        .filter(Boolean)
        .join(' ');

    // %B of the latest candle
    const lastBand = bollinger(closes, closes.length - 1);
    const lastClose = closes[closes.length - 1];
    const pctB =
      lastBand && lastBand.upper > lastBand.lower
        ? (lastClose - lastBand.lower) / (lastBand.upper - lastBand.lower)
        : null;

    // Fibonacci retracement over the visible swing (high → low).
    let pHi = -Infinity;
    let pLo = Infinity;
    for (const c of last) {
      pHi = Math.max(pHi, c.high);
      pLo = Math.min(pLo, c.low);
    }
    const fibRatios = [0, 0.236, 0.382, 0.5, 0.618, 0.786, 1];
    const fib =
      pHi > pLo ? fibRatios.map((r) => ({ r, price: pHi - r * (pHi - pLo) })) : [];

    // Volume sub-panel
    const volMax = Math.max(...last.map((c) => c.volume || 0), 1e-12);
    const yVol = (v: number) => H - (Math.max(0, v) / volMax) * volH;

    // Right-edge price axis ticks
    const tickCount = 4;
    const priceTicks = Array.from({ length: tickCount + 1 }, (_, k) => {
      const p = lo + (span * k) / tickCount;
      return { p, yy: y(p) };
    });

    const lastUp = last[last.length - 1].close >= last[last.length - 1].open;

    return {
      last, bands, W, H, x, y, cw, line, pctB, lastClose, fib, padX, padRight,
      yVol, priceTicks, priceBottom, lastUp, plotEndX: W - padRight,
    };
  }, [candles]);

  const pctB = view?.pctB ?? null;
  const deployable = pctB != null && pctB >= BB_MIN;

  return (
    <GlassCard className="chart-card">
      <div className="chart-head">
        <div className="chart-title">
          <CandlestickChart size={16} />
          <strong>{slot?.name ?? 'Empty slot'}</strong>
          {slot ? (
            <span className={`chart-tag ${slot.source}`}>
              {slot.source === 'position' ? 'POSITION' : <><Radar size={10} /> RADAR</>}
            </span>
          ) : null}
        </div>
        {pctB != null ? (
          <div className={`chart-signal ${deployable ? 'go' : 'wait'}`}>
            <span className="bb-label">%B</span>
            <strong>{pctB.toFixed(2)}</strong>
            <em>{deployable ? 'DEPLOY' : 'wait'}</em>
          </div>
        ) : null}
      </div>

      {view ? (
        <svg className="chart-svg" viewBox={`0 0 ${view.W} ${view.H}`} preserveAspectRatio="none">
          {/* Price grid + right axis */}
          {view.priceTicks.map((t) => (
            <g key={t.p} className="px-axis">
              <line x1={view.padX} x2={view.plotEndX} y1={t.yy} y2={t.yy} className="grid-line" />
              <text x={view.plotEndX + 3} y={t.yy + 2.5} className="px-label">{fmtPrice(t.p)}</text>
            </g>
          ))}
          {/* Bollinger Bands */}
          <polyline className="bb-upper" points={view.line('upper')} fill="none" />
          <polyline className="bb-mid" points={view.line('mid')} fill="none" />
          <polyline className="bb-lower" points={view.line('lower')} fill="none" />
          {/* Fibonacci retracement — golden zone (.382/.5/.618) emphasised */}
          {view.fib.map((f) => {
            const yy = view.y(f.price);
            const key = f.r === 0.382 || f.r === 0.5 || f.r === 0.618;
            return (
              <g key={f.r} className={`fib ${key ? 'key' : ''}`}>
                <line x1={view.padX} x2={view.plotEndX} y1={yy} y2={yy} className="fib-line" />
                {key ? (
                  <text x={view.plotEndX - 2} y={yy - 1.5} className="fib-label" textAnchor="end">
                    {f.r.toFixed(3).replace(/0+$/, '').replace(/\.$/, '')}
                  </text>
                ) : null}
              </g>
            );
          })}
          {/* Volume bars */}
          {view.last.map((c, i) => (
            <rect
              key={`v${c.time ?? i}`}
              x={view.x(i) - view.cw / 2}
              y={view.yVol(c.volume || 0)}
              width={view.cw}
              height={Math.max(0, view.H - view.yVol(c.volume || 0))}
              className={c.close >= c.open ? 'vol up' : 'vol down'}
            />
          ))}
          {/* Candles */}
          {view.last.map((c, i) => {
            const up = c.close >= c.open;
            const bodyTop = view.y(Math.max(c.open, c.close));
            const bodyBot = view.y(Math.min(c.open, c.close));
            return (
              <g key={c.time ?? i} className={up ? 'candle up' : 'candle down'}>
                <line x1={view.x(i)} x2={view.x(i)} y1={view.y(c.high)} y2={view.y(c.low)} className="wick" />
                <rect
                  x={view.x(i) - view.cw / 2}
                  y={bodyTop}
                  width={view.cw}
                  height={Math.max(0.8, bodyBot - bodyTop)}
                  className="body"
                />
              </g>
            );
          })}
          {/* Current price line + tag */}
          <line
            x1={view.padX}
            x2={view.plotEndX}
            y1={view.y(view.lastClose)}
            y2={view.y(view.lastClose)}
            className={`last-line ${view.lastUp ? 'up' : 'down'}`}
          />
          <rect
            x={view.plotEndX}
            y={view.y(view.lastClose) - 6}
            width={view.padRight}
            height={12}
            className={`last-tag ${view.lastUp ? 'up' : 'down'}`}
          />
          <text
            x={view.plotEndX + 3}
            y={view.y(view.lastClose) + 2.6}
            className="last-tag-text"
          >
            {fmtPrice(view.lastClose)}
          </text>
        </svg>
      ) : (
        <div className="chart-empty">
          {status === 'loading' && 'Loading chart…'}
          {status === 'empty' && `No open position · slot ${index + 1}`}
          {status === 'error' && 'Chart data unavailable'}
          {status === 'idle' && '—'}
          {status === 'ok' && 'Not enough candles'}
        </div>
      )}

      <div className="chart-foot">
        <span>{slot ? `${slot.mint.slice(0, 4)}…${slot.mint.slice(-4)}` : 'no token'}</span>
        <span>{view ? fmtPrice(view.lastClose) : ''} · BB(20,2) 5m</span>
      </div>
    </GlassCard>
  );
};
