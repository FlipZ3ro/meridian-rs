// Sign-In with Solana (SIWS) auth helpers. The user connects a wallet, signs a
// nonce message, and the backend issues an HMAC-signed session cookie. Used by
// the /api/auth/* routes (Node) and middleware (Edge) — so it relies only on
// Web Crypto, which exists in both runtimes.

export const COOKIE_NAME = 'meridian_session';
const DEFAULT_TTL_SEC = 60 * 60 * 12; // 12h session

const enc = new TextEncoder();

const b64url = (bytes: Uint8Array): string => {
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
};
const b64urlToBytes = (s: string): Uint8Array => {
  const pad = s.length % 4 === 0 ? '' : '='.repeat(4 - (s.length % 4));
  const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/') + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) out[i] = bin.charCodeAt(i);
  return out;
};

const authSecret = () =>
  process.env.AUTH_SECRET || 'meridian-dev-secret-change-me-in-production';

let cachedKey: CryptoKey | null = null;
const hmacKey = async (): Promise<CryptoKey> => {
  if (cachedKey) return cachedKey;
  cachedKey = await crypto.subtle.importKey(
    'raw',
    enc.encode(authSecret()) as BufferSource,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign', 'verify'],
  );
  return cachedKey;
};

type SessionPayload = { pubkey: string; exp: number };

export const signSession = async (pubkey: string, ttlSec = DEFAULT_TTL_SEC): Promise<string> => {
  const payload: SessionPayload = { pubkey, exp: Math.floor(Date.now() / 1000) + ttlSec };
  const data = b64url(enc.encode(JSON.stringify(payload)));
  const sig = new Uint8Array(await crypto.subtle.sign('HMAC', await hmacKey(), enc.encode(data) as BufferSource));
  return `${data}.${b64url(sig)}`;
};

export const verifySession = async (token?: string | null): Promise<SessionPayload | null> => {
  if (!token || !token.includes('.')) return null;
  const [data, sig] = token.split('.');
  try {
    const ok = await crypto.subtle.verify('HMAC', await hmacKey(), b64urlToBytes(sig) as BufferSource, enc.encode(data) as BufferSource);
    if (!ok) return null;
    const payload = JSON.parse(new TextDecoder().decode(b64urlToBytes(data))) as SessionPayload;
    if (!payload.exp || payload.exp < Math.floor(Date.now() / 1000)) return null;
    return payload;
  } catch {
    return null;
  }
};

// Allowlist of wallet pubkeys permitted to sign in via SIWS. Secure by default:
// an empty allowlist means wallet login is DISABLED (not "allow any"). To enable
// wallet login, set AUTH_ALLOWED_PUBKEYS to your wallet(s). Password login
// (AUTH_PASSWORD) is the primary gate and is independent of this.
export const allowedPubkeys = (): string[] =>
  (process.env.AUTH_ALLOWED_PUBKEYS || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);

export const isAllowed = (pubkey: string): boolean => {
  const list = allowedPubkeys();
  return list.length > 0 && list.includes(pubkey);
};

// Password login (works on any browser, incl. mobile where wallet injection is
// unavailable). The password is stored server-side in AUTH_PASSWORD (.env.local,
// root-only, gitignored). Constant-time compare to resist timing attacks. If
// AUTH_PASSWORD is unset, password login is disabled (returns false).
export const passwordLoginEnabled = (): boolean => Boolean(process.env.AUTH_PASSWORD);

// Constant-time string compare (always scans the longer length, folds in any
// length mismatch) to avoid leaking via timing.
const constantTimeEq = (input: string, expected: string): boolean => {
  if (!expected || !input) return false;
  const a = enc.encode(input);
  const b = enc.encode(expected);
  let diff = a.length ^ b.length;
  const max = Math.max(a.length, b.length);
  for (let i = 0; i < max; i += 1) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
};

export const checkPassword = (input: string): boolean =>
  constantTimeEq(input, process.env.AUTH_PASSWORD || '');

// Username check. If AUTH_USERNAME is unset, the username isn't required
// (back-compat with password-only deployments).
export const checkUsername = (input: string): boolean => {
  const expected = process.env.AUTH_USERNAME || '';
  if (!expected) return true;
  return constantTimeEq(input, expected);
};

// Verify both, computing each side regardless so the "which field was wrong"
// isn't leaked by short-circuit timing.
export const checkLogin = (username: string, password: string): boolean => {
  const okUser = checkUsername(username);
  const okPass = checkPassword(password);
  return okUser && okPass;
};

// Human-readable message the wallet signs (includes the nonce).
export const buildSignMessage = (nonce: string): string =>
  `Sign in to Meridian DLMM Agent\n\nThis request will not trigger a blockchain transaction or cost gas.\n\nNonce: ${nonce}`;

// In-memory nonce store (single long-running Next server on the VPS). Keyed by
// pubkey; one-time use, short TTL.
type NonceEntry = { nonce: string; exp: number };
const nonceStore: Map<string, NonceEntry> = (globalThis as any).__meridianNonces ?? new Map();
(globalThis as any).__meridianNonces = nonceStore;

export const issueNonce = (pubkey: string): string => {
  const nonce = b64url(crypto.getRandomValues(new Uint8Array(24)));
  nonceStore.set(pubkey, { nonce, exp: Date.now() + 5 * 60 * 1000 });
  return nonce;
};

export const consumeNonce = (pubkey: string): string | null => {
  const entry = nonceStore.get(pubkey);
  if (!entry || entry.exp < Date.now()) {
    nonceStore.delete(pubkey);
    return null;
  }
  nonceStore.delete(pubkey); // one-time use
  return entry.nonce;
};
