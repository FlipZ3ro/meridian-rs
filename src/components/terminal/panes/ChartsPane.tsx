'use client';

import { useEffect, useRef, useState } from 'react';
import { cachedJson } from '../../../lib/clientCache';

type Candle = { time: number; open: number; high: number; low: number; close: number; volume: number };
type Slot = { mint: string; name: string; source: 'position' };

const BB_PERIOD = 20;
const BB_MIN = 0.8;
const SLOTS = 4;

const candleCache = new Map<string, Candle[]>();

const fmtPrice = (n: number) => {
  if (!Number.isFinite(n) || n <= 0) return '—';
  if (n >= 1) return n.toFixed(4);
  const s = n.toFixed(12);
  const m = s.match(/^0\.(0+)(\d{1,4})/);
  if (m) return `0.0${m[1].length > 1 ? `(${m[1].length})` : ''}${m[2]}`;
  return n.toPrecision(3);
};

// Draw a candlestick + Bollinger(20,2) + fib + volume chart from real candles,
// styled to match the terminal mockup (right price axis, %B badge, last tag).
const draw = (canvas: HTMLCanvasElement, candles: Candle[]) => {
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const view = candles.slice(-76);
  const N = view.length;
  if (N < 2) return;
  const closes = candles.map((c) => c.close);
  const offset = candles.length - N;

  const band = (i: number) => {
    const abs = offset + i;
    if (abs < BB_PERIOD - 1) return null;
    const win = closes.slice(abs - BB_PERIOD + 1, abs + 1);
    const mid = win.reduce((a, b) => a + b, 0) / BB_PERIOD;
    const sd = Math.sqrt(win.reduce((a, b) => a + (b - mid) ** 2, 0) / BB_PERIOD);
    return { mid, upper: mid + 2 * sd, lower: mid - 2 * sd };
  };
  const bands = view.map((_, i) => band(i));

  const padR = 74;
  const padL = 6;
  const volH = Math.max(38, h * 0.17);
  const top = 10;
  const chartH = h - volH - top - 22;
  const plotW = w - padR - padL;

  let lo = Infinity;
  let hi = -Infinity;
  for (let i = 0; i < N; i++) {
    lo = Math.min(lo, view[i].low);
    hi = Math.max(hi, view[i].high);
    const b = bands[i];
    if (b) { lo = Math.min(lo, b.lower); hi = Math.max(hi, b.upper); }
  }
  if (!Number.isFinite(lo) || !Number.isFinite(hi) || hi <= lo) return;
  const pad = (hi - lo) * 0.06;
  lo -= pad; hi += pad;

  const y = (v: number) => top + chartH - ((v - lo) / (hi - lo)) * chartH;
  const x = (i: number) => padL + (i + 0.5) * (plotW / N);
  const bw = Math.max(2, (plotW / N) * 0.62);

  ctx.font = '9px "Geist Mono", "IBM Plex Mono", monospace';
  ctx.textBaseline = 'middle';

  // grid + price labels
  ctx.strokeStyle = '#111a26';
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 4]);
  for (let g = 0; g <= 4; g++) {
    const gv = lo + ((hi - lo) * g) / 4;
    const gy = Math.round(y(gv)) + 0.5;
    ctx.beginPath(); ctx.moveTo(padL, gy); ctx.lineTo(w - padR, gy); ctx.stroke();
    ctx.fillStyle = '#3f4a5b';
    ctx.textAlign = 'left';
    ctx.fillText(fmtPrice(gv), w - padR + 8, gy);
  }
  ctx.setLineDash([]);

  // fib retracement over visible swing
  let pHi = -Infinity;
  let pLo = Infinity;
  for (const c of view) { pHi = Math.max(pHi, c.high); pLo = Math.min(pLo, c.low); }
  ctx.strokeStyle = '#7a5a1c';
  for (const f of [0.382, 0.618]) {
    const fv = pLo + (pHi - pLo) * f;
    const fy = Math.round(y(fv)) + 0.5;
    ctx.beginPath(); ctx.moveTo(padL, fy); ctx.lineTo(w - padR, fy); ctx.stroke();
    ctx.fillStyle = '#ffd64a';
    ctx.textAlign = 'right';
    ctx.fillText(String(f), w - padR - 6, fy - 8);
  }

  // BB lines
  const line = (key: 'upper' | 'mid' | 'lower', color: string, dash?: number[]) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.setLineDash(dash || []);
    ctx.beginPath();
    let started = false;
    for (let i = 0; i < N; i++) {
      const b = bands[i];
      if (!b) continue;
      const px = x(i);
      const py = y(b[key]);
      if (!started) { ctx.moveTo(px, py); started = true; } else ctx.lineTo(px, py);
    }
    ctx.stroke();
    ctx.setLineDash([]);
  };
  line('upper', 'rgba(155,124,255,0.7)');
  line('lower', 'rgba(155,124,255,0.7)');
  line('mid', 'rgba(110,168,255,0.75)', [3, 3]);

  // candles
  for (let i = 0; i < N; i++) {
    const b = view[i];
    const bull = b.close >= b.open;
    const col = bull ? '#3ddc84' : '#ff5f7a';
    ctx.strokeStyle = col;
    ctx.fillStyle = col;
    ctx.lineWidth = 1;
    const cx = Math.round(x(i)) + 0.5;
    ctx.beginPath(); ctx.moveTo(cx, y(b.high)); ctx.lineTo(cx, y(b.low)); ctx.stroke();
    const yo = y(b.open);
    const yc = y(b.close);
    ctx.fillRect(cx - bw / 2, Math.min(yo, yc), bw, Math.max(1.5, Math.abs(yc - yo)));
  }

  // last price line + tag
  const last = view[N - 1];
  const ly = Math.round(y(last.close)) + 0.5;
  const bullLast = last.close >= last.open;
  ctx.strokeStyle = bullLast ? 'rgba(61,220,132,0.55)' : 'rgba(255,95,122,0.55)';
  ctx.setLineDash([3, 3]);
  ctx.beginPath(); ctx.moveTo(padL, ly); ctx.lineTo(w - padR, ly); ctx.stroke();
  ctx.setLineDash([]);
  ctx.fillStyle = bullLast ? '#3ddc84' : '#ff5f7a';
  ctx.fillRect(w - padR + 2, ly - 8, padR - 4, 16);
  ctx.fillStyle = '#04070c';
  ctx.textAlign = 'center';
  ctx.font = '700 9px "Geist Mono", "IBM Plex Mono", monospace';
  ctx.fillText(fmtPrice(last.close), w - padR + 2 + (padR - 4) / 2, ly);

  // volume
  let maxV = 0;
  for (let i = 0; i < N; i++) maxV = Math.max(maxV, view[i].volume || 0);
  maxV = maxV || 1;
  const vTop = h - volH - 14;
  for (let i = 0; i < N; i++) {
    const b = view[i];
    const vh = Math.max(1, ((b.volume || 0) / maxV) * (volH - 8));
    ctx.fillStyle = b.close >= b.open ? 'rgba(61,220,132,0.42)' : 'rgba(255,95,122,0.42)';
    ctx.fillRect(Math.round(x(i)) - bw / 2, vTop + (volH - 8) - vh, bw, vh);
  }
  ctx.fillStyle = '#2e3a4c';
  ctx.textAlign = 'left';
  ctx.font = '9px "Geist Mono", "IBM Plex Mono", monospace';
  ctx.fillText('VOL', padL + 2, vTop + 6);
};

const pctBOf = (candles: Candle[]): number | null => {
  const closes = candles.map((c) => c.close);
  if (closes.length < BB_PERIOD) return null;
  const win = closes.slice(-BB_PERIOD);
  const mid = win.reduce((a, b) => a + b, 0) / BB_PERIOD;
  const sd = Math.sqrt(win.reduce((a, b) => a + (b - mid) ** 2, 0) / BB_PERIOD);
  const upper = mid + 2 * sd;
  const lower = mid - 2 * sd;
  if (upper <= lower) return null;
  return (closes[closes.length - 1] - lower) / (upper - lower);
};

const TerminalChart = ({ slot }: { slot: Slot }) => {
  const ref = useRef<HTMLCanvasElement>(null);
  const [candles, setCandles] = useState<Candle[]>(() => candleCache.get(slot.mint) ?? []);

  useEffect(() => {
    let mounted = true;
    const cached = candleCache.get(slot.mint);
    if (cached?.length) setCandles(cached);
    const load = async () => {
      try {
        const payload = await cachedJson<{ candles: Candle[] }>(`/api/chart/${slot.mint}?interval=5_MINUTE&candles=120`, 20_000);
        const list = (payload?.candles ?? []).filter((c) => c && c.close > 0);
        if (!mounted) return;
        if (list.length) { candleCache.set(slot.mint, list); setCandles(list); }
      } catch { /* keep cached */ }
    };
    load();
    const t = window.setInterval(load, 20_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, [slot.mint]);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || candles.length < 2) return;
    let raf = 0;
    const paint = () => draw(canvas, candles);
    raf = window.requestAnimationFrame(paint);
    const onResize = () => { window.cancelAnimationFrame(raf); raf = window.requestAnimationFrame(paint); };
    window.addEventListener('resize', onResize);
    return () => { window.cancelAnimationFrame(raf); window.removeEventListener('resize', onResize); };
  }, [candles]);

  const pctB = pctBOf(candles);
  const deployable = pctB != null && pctB >= BB_MIN;
  const addr = `${slot.mint.slice(0, 4)}…${slot.mint.slice(-4)}`;

  return (
    <div className="mrd-chart">
      <div className="mrd-chart-head">
        <span className="name">{slot.name}</span>
        <span className="mrd-chart-tag">POSITION</span>
        <span className="grow" />
        <span className="bk">%B</span>
        <span className="pb" style={{ color: deployable ? 'var(--green)' : 'var(--amber)' }}>{pctB != null ? pctB.toFixed(2) : '—'}</span>
        <span className={`mrd-chart-sig ${deployable ? 'go' : ''}`}>{deployable ? 'DEPLOY' : 'WAIT'}</span>
      </div>
      <canvas ref={ref} />
      <div className="mrd-chart-foot">
        <span>{addr}</span>
        <span>BB(20,2) 5m</span>
      </div>
    </div>
  );
};

const EmptySlot = ({ index }: { index: number }) => (
  <div className="mrd-chart empty">
    <div className="mrd-chart-head"><span className="name">Empty slot</span></div>
    <div className="mrd-chart-empty-body">No open position · slot {index + 1}</div>
    <div className="mrd-chart-foot"><span>no token</span><span>· BB(20,2) 5m</span></div>
  </div>
);

// Charts track ONLY live open positions. The mapped PositionRow doesn't carry the
// raw base mint (which /api/chart is keyed by), so this pane reads the positions
// endpoint directly to build its slots.
export const ChartsPane = () => {
  const [slots, setSlots] = useState<(Slot | null)[]>([null, null, null, null]);
  const [count, setCount] = useState(0);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const payload = await cachedJson<any>('/api/meridian/positions', 8_000);
        if (!Array.isArray(payload?.data?.positions)) return;
        const out: Slot[] = [];
        const seen = new Set<string>();
        for (const p of payload.data.positions) {
          if (String(p?.status ?? 'active').toLowerCase() === 'closed') continue;
          const mint = p?.base_mint;
          if (!mint || seen.has(mint)) continue;
          seen.add(mint);
          out.push({ mint, name: String(p?.pool_name ?? p?.base_symbol ?? 'TOKEN'), source: 'position' });
        }
        const next: (Slot | null)[] = out.slice(0, SLOTS);
        while (next.length < SLOTS) next.push(null);
        if (mounted) { setSlots(next); setCount(out.length); }
      } catch { /* keep previous */ }
    };
    load();
    const t = window.setInterval(load, 15_000);
    return () => { mounted = false; window.clearInterval(t); };
  }, []);

  return (
    <div className="mrd-charts">
      <div className="mrd-charts-head">
        <span className="title">LIVE CHARTS — BOLLINGER %B</span>
        <span className="sub">{count > 0 ? `${count} open position${count > 1 ? 's' : ''} tracked` : 'no open positions'} · BB(20,2) 5m · entry %B ≥ 0.8</span>
      </div>
      <div className="mrd-charts-grid">
        {slots.map((slot, i) => slot ? <TerminalChart key={slot.mint} slot={slot} /> : <EmptySlot key={`empty-${i}`} index={i} />)}
      </div>
    </div>
  );
};
