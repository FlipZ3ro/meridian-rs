'use client';

import { useState } from 'react';
import { useAgentControl, useQuickFlip, useStatus } from '../hooks';
import { shortAddr } from '../../../lib/meridianFormat';

type Notify = (kind: string, text: string) => void;

export const SettingsPane = ({ onNotify }: { onNotify?: Notify }) => {
  const agent = useAgentControl();
  const flip = useQuickFlip();
  const status = useStatus();
  const [copied, setCopied] = useState(false);

  const online = agent.online;
  const agentLabel = online ? 'RUNNING' : agent.status === 'stopped' ? 'STOPPED' : agent.status.toUpperCase();
  const agentColor = online ? 'var(--green)' : 'var(--red)';

  const runAgent = async (action: 'start' | 'stop' | 'restart') => {
    if (action === 'stop' && !window.confirm('Stop the trading agent? It will stop screening and managing positions until you start it again.')) return;
    const res = await agent.act(action);
    onNotify?.(res.ok ? 'out' : 'err', res.ok ? `meridian-backend → ${res.status ?? action}` : 'agent control failed');
  };

  const toggleFlip = async () => {
    const next = !flip.armed;
    if (next && flip.live) {
      const size = flip.state?.params?.deploy_amount_sol ?? 0.2;
      if (!window.confirm(`Arm quick-flip in LIVE mode? The bot will deploy REAL positions (~${size} SOL each) on qualifying volume spikes.`)) return;
    }
    const res = await flip.setEnabled(next);
    if (res.ok) onNotify?.(next ? 'warn' : 'out', next ? 'quick-flip ARMED' : 'quick-flip disarmed');
  };

  const wallet: string | undefined = status?.wallet;
  const copyWallet = async () => {
    if (!wallet) return;
    try {
      await navigator.clipboard.writeText(wallet);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1300);
    } catch { /* clipboard unavailable */ }
  };

  const armed = flip.armed;
  const p = flip.state?.params;

  return (
    <div className="mrd-settings">
      {/* Agent control */}
      <section className="mrd-set-card">
        <span className="mrd-panel-label" style={{ background: 'var(--panel)' }}>AGENT CONTROL</span>
        <div className="mrd-set-state">
          <span className="sdot" style={{ background: agentColor }} />
          <span className="slabel" style={{ color: agentColor }}>{agentLabel}</span>
          <span className="grow" />
          <span className="sub">meridian-backend</span>
        </div>
        <div className="mrd-set-actions">
          <button type="button" aria-label="Start trading agent" className={`mrd-set-btn ${online ? '' : 'go'}`} disabled={agent.busy || online} onClick={() => runAgent('start')}>▷ START</button>
          <button type="button" aria-label="Stop trading agent" className={`mrd-set-btn ${online ? 'stop' : ''}`} disabled={agent.busy || !online} onClick={() => runAgent('stop')}>□ STOP</button>
          <button type="button" aria-label="Restart trading agent" className="mrd-set-btn" disabled={agent.busy} onClick={() => runAgent('restart')}>↻ RESTART</button>
        </div>
        <p className="mrd-set-note">Frontend &amp; dashboard stay online — only the trading agent starts/stops.</p>
      </section>

      {/* Quick-flip scalper */}
      <section className="mrd-set-card">
        <span className="mrd-panel-label" style={{ background: 'var(--panel)' }}>QUICK-FLIP SCALPER</span>
        <div className="mrd-set-state">
          <span className="sdot" style={{ background: armed ? 'var(--green)' : 'var(--red)' }} />
          <span className="slabel" style={{ color: armed ? 'var(--green)' : 'var(--red)' }}>{flip.state === undefined ? '…' : armed ? 'ARMED' : 'OFF'}</span>
          <span className="grow" />
          <span className="live">{flip.state?.mode ? (flip.live ? 'LIVE' : 'DRY RUN') : 'VOLUME-SPIKE'}</span>
        </div>
        <button
          type="button"
          aria-label={armed ? 'Disarm quick-flip scalper' : 'Arm quick-flip scalper'}
          className={`mrd-set-btn wide ${armed ? 'stop' : 'go'}`}
          disabled={flip.busy || flip.state === undefined}
          onClick={toggleFlip}
        >
          {armed ? '□ DISARM' : '▷ ARM'}
        </button>
        <div className="mrd-set-grid">
          <div className="mrd-set-kv"><div className="k">ENTRY VOL/MIN</div><div className="v">{p?.min_vol_per_min ? `$${(p.min_vol_per_min / 1000).toFixed(0)}k` : '-'}</div></div>
          <div className="mrd-set-kv"><div className="k">MAX HOLD</div><div className="v">{p?.max_hold_min ? `${p.max_hold_min} min` : '-'}</div></div>
          <div className="mrd-set-kv"><div className="k">FADE EXIT</div><div className="v">{p?.vol_fade_ratio ? `×${p.vol_fade_ratio}` : '-'}</div></div>
          <div className="mrd-set-kv"><div className="k">SIZE / POS</div><div className="v">{p?.deploy_amount_sol ? `◎${p.deploy_amount_sol}` : '-'}</div></div>
        </div>
        <p className="mrd-set-note">Deterministic, no LLM. Enters on volume spikes, exits on fade / max-hold. Single-side SOL.</p>
      </section>

      {/* Backend status */}
      <section className="mrd-set-card">
        <span className="mrd-panel-label" style={{ background: 'var(--panel)' }}>BACKEND STATUS</span>
        <div className="mrd-set-state">
          <span className="slabel" style={{ color: 'var(--green)' }}>{status?.status ?? 'loading'}</span>
          <span className="grow" />
          <span className="live">{status?.dry_run ? 'DRY RUN' : 'LIVE'}</span>
        </div>
        {wallet ? (
          <button type="button" className="mrd-set-wallet" aria-label="Copy wallet address" onClick={copyWallet} title={`${wallet}\nClick to copy`}>
            <span className="icon" aria-hidden="true">◧</span>
            <span>{shortAddr(wallet)}</span>
            <span className={`copy ${copied ? 'done' : ''}`}>{copied ? 'COPIED' : 'COPY'}</span>
          </button>
        ) : null}
        <div className="mrd-set-grid">
          <div className="mrd-set-kv"><div className="k">ACTIVE POSITIONS</div><div className="v big">{status?.active_positions ?? 0}</div></div>
          <div className="mrd-set-kv"><div className="k">SCREEN EVERY</div><div className="v big">{status?.schedule?.screeningIntervalMin ?? '-'} min</div></div>
          <div className="mrd-set-kv"><div className="k">MANAGE EVERY</div><div className="v big">{status?.schedule?.managementIntervalMin ?? '-'} min</div></div>
          <div className="mrd-set-kv"><div className="k">PNL POLL</div><div className="v big">{status?.schedule?.pnlPollIntervalSecs ?? '-'} sec</div></div>
          <div className="mrd-set-kv"><div className="k">STATE</div><div className="v big">{status?.state_path ? 'connected' : 'not set'}</div></div>
          <div className="mrd-set-kv"><div className="k">DATA DIR</div><div className="v big">{status?.data_dir ? 'available' : 'unknown'}</div></div>
        </div>
      </section>
    </div>
  );
};
