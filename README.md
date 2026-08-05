# Meridian Web UI

Hyprland-style operator dashboard for the Meridian DLMM agent. This branch packages a Next.js frontend at the repository root and the Rust Meridian backend under `backend/`.

The intended local setup is:

- Frontend dashboard: `http://127.0.0.1:3000`
- Rust API service: `http://127.0.0.1:3001`
- Frontend proxy: `/api/meridian/*` -> backend `/api/*`

## What Is Included

- Next.js 16 dashboard with draggable/resizable workspaces.
- Terminal/Hyprland visual system with wallpaper, dock, topbar, widgets, and terminal overlay.
- Open Positions UI backed by the Rust API and price proxies.
- Recent Trades event feed from backend position state.
- Candidate radar, activity log, weather, system stats, and music widgets.
- Frontend backend-control widgets for status, manual actions, config, wallet/decisions, lessons/performance/blacklist, and API reference.
- Rust backend moved into `backend/` and kept API-only on port `3001`.
- Dry-run first backend behavior with simulated positions, PnL, fees, recent events, and guardrail metadata.

## Repository Layout

```text
.
├─ src/                         Next.js app, components, widgets, styles
├─ public/                      Dashboard images and wallpaper
├─ scripts/                     Windows PowerShell helpers
├─ backend/                     Rust Meridian backend service
│  ├─ src/                      Agent, DLMM tools, web API, state, config
│  ├─ docs/                     Backend docs
│  ├─ .env.example              Backend environment template, no secrets
│  └─ user-config.example.json  Backend config template
├─ package.json                 Frontend and orchestration scripts
└─ tsconfig.json
```

## Safety

Keep `dryRun` / `DRY_RUN` enabled while testing.

Do not commit real values for:

- `WALLET_PRIVATE_KEY`
- `MERIDIAN_WALLET_PRIVATE_KEY`
- `HELIUS_API_KEY`
- `HELIUS_RPC_URL` if it contains a key
- `LLM_API_KEY`
- `OPENROUTER_API_KEY`
- `PUBLIC_API_KEY`
- `LPAGENT_API_KEY`
- Telegram or Discord tokens

The repository ignores local env files, backend runtime state, build output, and dependency folders.

## Requirements

- Node.js 20+
- npm
- Rust stable toolchain
- Windows PowerShell 5.1+
- Strawberry Perl on Windows for vendored OpenSSL builds

The included build scripts add the common Strawberry Perl path when building the backend.

## Quickstart

```powershell
git clone https://github.com/FlipZ3ro/meridian-rs.git
cd meridian-rs
git switch web-ui

npm install
Copy-Item "backend\.env.example" "backend\.env"
Copy-Item "backend\user-config.example.json" "backend\user-config.json"
```

Edit local files only:

```powershell
notepad "backend\.env"
notepad "backend\user-config.json"
```

For testing, keep:

```json
"dryRun": true
```

Run frontend and backend together:

```powershell
npm run dev:all
```

Open:

```text
http://127.0.0.1:3000
```

## Scripts

```powershell
npm run dev              # Next.js frontend only
npm run backend:dev      # Rust backend only, port 3001
npm run dev:all          # Backend detached + Next dev server
npm run build            # Next production build
npm run build:backend    # Rust backend build
npm run build:all        # Backend build + frontend build
npm run start            # Next production server
```

## Backend API

The backend is API-only. The root backend page points users back to the frontend dashboard.

Common endpoints:

```text
GET  /api/status
GET  /api/positions
GET  /api/candidates?limit=5
GET  /api/decisions
GET  /api/config
POST /api/config
POST /api/control
GET  /api/lessons
GET  /api/performance
GET  /api/blacklist
```

From the frontend, use the proxy path:

```text
/api/meridian/status
/api/meridian/positions
/api/meridian/candidates?limit=5
```

The proxy target defaults to:

```text
http://127.0.0.1:3001
```

Set `MERIDIAN_BACKEND_URL` in `.env.local` if you need a different backend URL.

## Development Notes

- The frontend owns the operator UI.
- Backend port `3001` is an API service, not the dashboard.
- Existing Rust backend docs live in `backend/README.md`.
- Backend state is stored in the backend data directory and should not be committed.
- Dry-run deploys can create simulated `dryrun-*` positions.
- Simulated dry-run PnL and fees update through `/api/positions`.
- Recent Trades reads backend `recent_events` from `/api/meridian/positions`.

## Verification

Recommended checks before pushing:

```powershell
cd backend
cargo test -- --test-threads=1
cd ..
npm run build:backend
npm run build
```

Expected current status on this branch:

```text
cargo test -- --test-threads=1  -> 153 passed
npm run build:backend           -> success
npm run build                   -> success
```

## Troubleshooting

If backend build fails because `meridian-rs.exe` is locked, stop the process listening on port `3001` and rebuild.

If backend refuses to start because of a stale lock, stop the old process and remove:

```text
backend/.meridian/meridian.lock
```

If Next shows a hydration warning with `bis_skin_checked`, that is usually a browser extension mutating localhost HTML. The dashboard uses a client hydration guard, but disabling the extension for localhost can still help.

If `/api/meridian/status` fails, check that the backend is running on `127.0.0.1:3001`.

## Branch

This README describes the `web-ui` branch.
