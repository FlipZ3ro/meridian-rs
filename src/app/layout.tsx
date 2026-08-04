import '../styles/theme.css';
import '../styles/terminal.css';
import type { Metadata } from 'next';
import type React from 'react';

export const metadata: Metadata = {
  title: 'Meridian — DLMM Agent',
  description: 'Terminal dashboard for the Meridian DLMM trading agent.',
  // This dashboard is reachable from the internet and shows wallet state, so it
  // must never end up in a search index.
  robots: { index: false, follow: false, nocache: true },
  icons: { icon: '/favicon.svg' },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body suppressHydrationWarning>{children}</body>
    </html>
  );
}
