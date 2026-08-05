'use client';

import { useState } from 'react';
import { Layers, History } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';
import { PositionTable } from './PositionTable';
import { PortfolioWidget } from './PortfolioWidget';

// Overview-only combined card: one panel with Open | Closed tabs. "Open" shows
// the live positions (short), "Closed" shows the historical summary + closed
// table (can be long → scrolls inside the panel on desktop). The dedicated
// Positions / Portfolio nav views still render the full standalone widgets.
export const PositionsCard = () => {
  const [tab, setTab] = useState<'open' | 'closed'>('open');

  return (
    <GlassCard className="positions-card terminal-positions positions-tabbed">
      <div className="positions-tabs">
        <button type="button" className={tab === 'open' ? 'active' : ''} onClick={() => setTab('open')}>
          <Layers size={17} /> Open
        </button>
        <button type="button" className={tab === 'closed' ? 'active' : ''} onClick={() => setTab('closed')}>
          <History size={17} /> Closed
        </button>
      </div>
      <div className="positions-tab-body">
        {tab === 'open' ? <PositionTable /> : <PortfolioWidget />}
      </div>
    </GlassCard>
  );
};
