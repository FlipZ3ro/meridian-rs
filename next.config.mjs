/** @type {import('next').NextConfig} */

// Security headers applied to every response. CSP is intentionally omitted for
// now — the app uses Google Fonts @import, inline styles, and the Phantom
// wallet injection, so a strict CSP needs separate tuning to avoid breakage.
const securityHeaders = [
  // Force HTTPS for two years (the site is HTTPS-only via Cloudflare).
  { key: 'Strict-Transport-Security', value: 'max-age=63072000; includeSubDomains; preload' },
  // Disallow framing — prevents clickjacking of the dashboard/lock screen.
  { key: 'X-Frame-Options', value: 'DENY' },
  // Stop MIME sniffing.
  { key: 'X-Content-Type-Options', value: 'nosniff' },
  // Don't leak full URLs in the Referer to other origins.
  { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
  // Drop unused powerful features.
  { key: 'Permissions-Policy', value: 'camera=(), microphone=(), geolocation=(), interest-cohort=()' },
];

const nextConfig = {
  poweredByHeader: false, // hide "X-Powered-By: Next.js"
  async headers() {
    return [{ source: '/:path*', headers: securityHeaders }];
  },
};

export default nextConfig;
