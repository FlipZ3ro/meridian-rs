'use client';

import { useState } from 'react';
import { letterFor } from '../../lib/meridianFormat';

// A 16px round token image that walks through fallback sources on load error,
// then renders a lettered avatar so a token is always identifiable.
export const TokenLogo = ({ srcs, alt }: { srcs: string[]; alt: string }) => {
  const [idx, setIdx] = useState(0);
  const src = srcs[idx];
  if (!src) {
    return <i className="mrd-logo-letter">{letterFor(alt)}</i>;
  }
  return (
    <img
      src={src}
      alt={alt}
      width={16}
      height={16}
      loading="lazy"
      onError={() => setIdx((i) => i + 1)}
      className="mrd-logo-img"
    />
  );
};
