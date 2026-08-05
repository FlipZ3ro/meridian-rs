'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { LockScreen } from './LockScreen';

// Gates the dashboard behind SIWS login. Checks the session on mount; shows the
// lock screen until authenticated. (API routes are independently protected by
// middleware, so this is the UX layer — the data/control endpoints stay locked
// regardless.)
export const AuthGate = ({ children }: { children: ReactNode }) => {
  const [state, setState] = useState<'checking' | 'locked' | 'authed'>('checking');

  const check = async () => {
    try {
      const res = await fetch('/api/auth/me', { cache: 'no-store' });
      const data = await res.json();
      setState(data?.authed ? 'authed' : 'locked');
    } catch {
      setState('locked');
    }
  };

  useEffect(() => {
    check();
  }, []);

  if (state === 'checking') {
    return <div className="lock-screen"><div className="lock-checking">Unlocking…</div></div>;
  }
  if (state === 'locked') {
    return <LockScreen onAuthed={() => setState('authed')} />;
  }
  return <>{children}</>;
};
