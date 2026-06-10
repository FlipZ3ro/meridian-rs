'use client';

type CacheEntry = {
  expiresAt: number;
  promise: Promise<unknown>;
};

const cache = new Map<string, CacheEntry>();

export const cachedJson = async <T,>(url: string, ttlMs: number): Promise<T> => {
  const now = Date.now();
  const existing = cache.get(url);
  if (existing && existing.expiresAt > now) return existing.promise as Promise<T>;

  const promise = fetch(url, { cache: 'no-store' }).then((response) => response.json());
  cache.set(url, { expiresAt: now + ttlMs, promise });

  try {
    return await promise as T;
  } catch (error) {
    cache.delete(url);
    throw error;
  }
};
