# Start the trading agent in DRY RUN.
#
# Differs from start-backend.ps1 in two ways that matter on Windows:
#
#   1. DRY_RUN=true is set in the process environment BEFORE the binary runs.
#      The agent loads its .env through dotenvy::from_path, which does not
#      override variables that are already set — so this wins over a
#      DRY_RUN=false sitting in the profile.
#
#   2. MERIDIAN_HOME is set explicitly. The Rust side resolves its home via the
#      HOME variable, which Windows does not define, so it silently falls back
#      to "." and never finds ~/.meridian/.env — wallet and RPC then come up
#      missing at startup.
#
# Runtime state is written to backend/.meridian so a dry run cannot touch the
# live profile's state file.

$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Backend = Join-Path $Root 'backend'
$Runtime = Join-Path $Backend '.meridian'

New-Item -ItemType Directory -Force -Path $Runtime | Out-Null

# Credentials profile. Prefer the repo-local backend/.env when present, else the
# installed profile at ~/.meridian.
$HomeProfile = Join-Path $env:USERPROFILE '.meridian'
$EnvFile = Join-Path $Backend '.env'
if (Test-Path -LiteralPath $EnvFile) {
  Get-Content -LiteralPath $EnvFile | ForEach-Object {
    if ($_ -match '^\s*([^#=][^=]*)=(.*)$') {
      [Environment]::SetEnvironmentVariable($matches[1].Trim(), $matches[2].Trim().Trim('"'), 'Process')
    }
  }
  $env:MERIDIAN_HOME = $Backend
} else {
  $env:MERIDIAN_HOME = $HomeProfile
}

# Safety flag last, so nothing loaded above can win over it.
$env:DRY_RUN = 'true'

$env:MERIDIAN_WEB_ADDR = '127.0.0.1:3001'
$env:MERIDIAN_DATA_DIR = $Runtime
$env:MERIDIAN_STATE_PATH = Join-Path $Runtime 'meridian-state.json'
$env:MERIDIAN_LOCK_PATH = Join-Path $Runtime 'meridian.lock'
$env:PATH = 'C:\Strawberry\perl\bin;C:\Strawberry\c\bin;' + $env:PATH

Write-Host "DRY_RUN=$env:DRY_RUN  MERIDIAN_HOME=$env:MERIDIAN_HOME  web=$env:MERIDIAN_WEB_ADDR"

Set-Location -LiteralPath $Backend
cargo run
