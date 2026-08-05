'use client';

import { useState } from 'react';
import bs58 from 'bs58';
import { Lock, Wallet, KeyRound } from 'lucide-react';
import { Clock } from '../layout/Clock';

type PhantomProvider = {
  isPhantom?: boolean;
  connect: () => Promise<{ publicKey: { toString: () => string } }>;
  signMessage: (message: Uint8Array, display?: string) => Promise<{ signature: Uint8Array }>;
};

const getProvider = (): PhantomProvider | null => {
  if (typeof window === 'undefined') return null;
  const anyWin = window as any;
  const p = anyWin.solana ?? anyWin.phantom?.solana;
  return p?.isPhantom ? p : p ?? null;
};

export const LockScreen = ({ onAuthed }: { onAuthed: (pubkey: string) => void }) => {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [pwStatus, setPwStatus] = useState<'idle' | 'verifying'>('idle');
  const [showWallet, setShowWallet] = useState(false);
  const [status, setStatus] = useState<'idle' | 'connecting' | 'signing' | 'verifying' | 'error'>('idle');
  const [error, setError] = useState('');

  const loginPassword = async () => {
    if (!username || !password || pwStatus === 'verifying') return;
    setError('');
    setPwStatus('verifying');
    try {
      const res = await fetch('/api/auth/login', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? 'login failed');
      onAuthed(username);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'login failed');
      setPwStatus('idle');
    }
  };

  const signIn = async () => {
    setError('');
    const provider = getProvider();
    if (!provider) {
      setError('Phantom wallet not found — use password, or open in Phantom browser.');
      setStatus('error');
      return;
    }
    try {
      setStatus('connecting');
      const { publicKey } = await provider.connect();
      const pubkey = publicKey.toString();

      const nonceRes = await fetch(`/api/auth/nonce?pubkey=${encodeURIComponent(pubkey)}`);
      const nonceData = await nonceRes.json();
      if (!nonceRes.ok) throw new Error(nonceData?.error ?? 'failed to get nonce');

      setStatus('signing');
      const signed = await provider.signMessage(new TextEncoder().encode(nonceData.message), 'utf8');
      const signature = bs58.encode(signed.signature);

      setStatus('verifying');
      const verifyRes = await fetch('/api/auth/verify', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ pubkey, signature }),
      });
      const verifyData = await verifyRes.json();
      if (!verifyRes.ok) throw new Error(verifyData?.error ?? 'verification failed');

      onAuthed(pubkey);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'sign-in failed');
      setStatus('error');
    }
  };

  const busy = status === 'connecting' || status === 'signing' || status === 'verifying';
  const walletLabel =
    status === 'connecting' ? 'Connecting…'
      : status === 'signing' ? 'Sign in your wallet…'
        : status === 'verifying' ? 'Verifying…'
          : 'Connect wallet';

  return (
    <div className="lock-screen">
      <div className="lock-clock"><Clock type="time" /><span><Clock type="date" /></span></div>
      <div className="lock-card">
        <div className="lock-avatar"><img src="/profile-avatar.png" alt="OxRapzz" /></div>
        <h1>OxRapzz</h1>
        <p className="lock-sub">Meridian DLMM Agent</p>

        <form
          className="lock-pw"
          onSubmit={(e) => { e.preventDefault(); loginPassword(); }}
        >
          <input
            type="text"
            className="lock-input"
            placeholder="Username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoFocus
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
          />
          <input
            type="password"
            className="lock-input"
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
          />
          <button type="submit" className="lock-btn" disabled={!username || !password || pwStatus === 'verifying'}>
            {pwStatus === 'verifying' ? <Lock size={16} /> : <KeyRound size={16} />}
            {pwStatus === 'verifying' ? 'Unlocking…' : 'Unlock'}
          </button>
        </form>

        {showWallet ? (
          <button type="button" className="lock-btn lock-btn-alt" onClick={signIn} disabled={busy}>
            {busy ? <Lock size={16} /> : <Wallet size={16} />} {walletLabel}
          </button>
        ) : (
          <button type="button" className="lock-link" onClick={() => setShowWallet(true)}>
            or connect wallet
          </button>
        )}

        {error ? <p className="lock-error">{error}</p> : <p className="lock-hint">Password works on any device — no gas</p>}
      </div>
    </div>
  );
};
