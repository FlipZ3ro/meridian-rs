'use client';

import { useEffect, useState } from 'react';
import { useSystem, useWeather, type MeridianStatus } from './hooks';

const SPIN = ['-', '\\', '|', '/'];
const NOW_PLAYING = 'HEAL (feat. Venes) — Weird Genius';

type Props = {
  agentOnline: boolean;
  agentLabel: string;
  status?: MeridianStatus;
};

export const StatusLine = ({ agentOnline, agentLabel, status }: Props) => {
  const system = useSystem();
  const weather = useWeather();
  const [tick, setTick] = useState(0);
  const [vw, setVw] = useState(typeof window === 'undefined' ? 1440 : window.innerWidth);

  useEffect(() => {
    const timer = window.setInterval(() => setTick((t) => t + 1), 1000);
    const onResize = () => setVw(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => { window.clearInterval(timer); window.removeEventListener('resize', onResize); };
  }, []);

  const now = new Date();
  const clock = now.toLocaleTimeString('en-US', { hour12: false });
  const date = now.toLocaleDateString('en-US', { month: 'numeric', day: 'numeric', year: 'numeric' });
  const agentColor = agentOnline ? 'var(--green)' : 'var(--red)';
  const ram = system.memory ? `${system.ramUsed}/${system.ramTotal}` : `${system.ramUsed}`;
  const showMedia = vw >= 1080;
  const weatherText = vw >= 1340 ? `${weather.temperature}°C ${weather.location}` : `${weather.temperature}°C`;

  return (
    <header className="mrd-status">
      <div className="st-group">
        <span className="mrd-logo" aria-hidden="true">M</span>
        <span className="mrd-brand">MERIDIAN</span>
        <span className="mrd-sep">/</span>
        <span className="mrd-subtle">DLMM AGENT</span>
      </div>
      <div className="st-group">
        <span className="mrd-dot" style={{ background: agentColor }} aria-hidden="true" />
        <span style={{ color: agentColor, fontWeight: 700, letterSpacing: '.14em' }} aria-label={`Agent ${agentLabel}`}>{agentLabel}</span>
      </div>
      <div className="mrd-ws">
        <span style={{ letterSpacing: '.14em' }}>ws</span>
        <span className="on">1</span>
        <span className="off">2</span>
      </div>
      <div className="mrd-spacer" />
      <span style={{ color: 'var(--purple)', fontSize: 12 }} aria-hidden="true">{SPIN[tick % SPIN.length]}</span>
      <div className="mrd-metric">
        <span className="k">CPU</span>
        <span className="v">{system.cpu}%</span>
        <span className="mrd-sep">·</span>
        <span className="k">RAM</span>
        <span className="v">{ram}</span>
      </div>
      {showMedia ? (
        <div className="mrd-media">
          <span style={{ flex: '0 0 auto', color: 'var(--fainter)' }}>│</span>
          <span style={{ flex: '0 0 auto', color: 'var(--purple)' }}>♪</span>
          <span className="m-note">{NOW_PLAYING}</span>
          <span style={{ flex: '0 0 auto', color: 'var(--blue)' }}>☁</span>
          <span style={{ flex: '0 0 auto', color: 'var(--soft)' }}>{weatherText}</span>
        </div>
      ) : null}
      <span className="mrd-sep" aria-hidden="true">│</span>
      <span className="mrd-clock">{clock}</span>
      <span className="mrd-date">{date}</span>
    </header>
  );
};
