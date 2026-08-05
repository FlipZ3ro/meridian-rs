'use client';

import { useEffect, useState } from 'react';
import { LineChart } from 'lucide-react';
import { ChartWidget, type ChartSlot } from '../widgets/ChartWidget';
import { cachedJson } from '../../lib/clientCache';

const SLOTS = 4;

// Module-level cache so switching tabs/widgets and coming back shows the last
// known chart slots instantly instead of resetting to empty while positions
// refetch.
let cachedSlots: (ChartSlot | null)[] = [null, null, null, null];
let cachedPosCount = 0;

// Charts track ONLY live open positions. No position → empty slot. One
// position → one chart, the rest stay empty (no radar auto-fill).
const buildSlots = (positions: any): (ChartSlot | null)[] => {
  const out: ChartSlot[] = [];
  const seen = new Set<string>();

  const posList = Array.isArray(positions?.data?.positions) ? positions.data.positions : [];
  for (const p of posList) {
    if (String(p?.status ?? 'active').toLowerCase() === 'closed') continue;
    const mint = p?.base_mint;
    if (!mint || seen.has(mint)) continue;
    seen.add(mint);
    out.push({ mint, name: (p?.pool_name ?? p?.base_symbol ?? 'TOKEN').toString(), source: 'position' });
  }

  const slots: (ChartSlot | null)[] = out.slice(0, SLOTS);
  while (slots.length < SLOTS) slots.push(null);
  return slots;
};

export const ChartGridView = () => {
  const [slots, setSlots] = useState<(ChartSlot | null)[]>(cachedSlots);
  const [posCount, setPosCount] = useState(cachedPosCount);

  useEffect(() => {
    let mounted = true;
    const load = async () => {
      try {
        const positions = await cachedJson<any>('/api/meridian/positions', 8_000).catch(() => null);
        if (!mounted) return;
        // Only update from an authoritative response; if positions couldn't be
        // fetched, keep the cached slots instead of clearing the grid.
        if (!Array.isArray(positions?.data?.positions)) return;
        const next = buildSlots(positions);
        cachedSlots = next;
        cachedPosCount = next.filter((s) => s?.source === 'position').length;
        setSlots(next);
        setPosCount(cachedPosCount);
      } catch {
        /* keep previous slots */
      }
    };
    load();
    const t = window.setInterval(load, 15_000);
    return () => {
      mounted = false;
      window.clearInterval(t);
    };
  }, []);

  return (
    <div className="chart-grid-view">
      <header className="chart-grid-head">
        <div><LineChart size={18} /><h2>Live Charts — Bollinger %B</h2></div>
        <span>
          {posCount > 0 ? `${posCount} open position${posCount > 1 ? 's' : ''} tracked` : 'no open positions'} · BB(20,2) 5m · entry %B ≥ 0.8
        </span>
      </header>
      <div className="chart-grid">
        {slots.map((slot, i) => (
          <ChartWidget key={slot?.mint ?? `empty-${i}`} slot={slot} index={i} />
        ))}
      </div>
    </div>
  );
};
