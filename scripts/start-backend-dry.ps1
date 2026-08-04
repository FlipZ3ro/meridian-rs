# Start the trading agent in DRY RUN.
#
# ASCII only, deliberately. Windows PowerShell 5.1 reads a BOM-less file as ANSI,
# so a UTF-8 em dash arrives as three bytes ending in a double quote and
# terminates the surrounding string early -- the script then fails to parse.
#
# Differs from start-backend.ps1 in three ways that matter on Windows:
#
#   1. DRY_RUN=true is set in the process environment AFTER the profile is
#      loaded, so it wins over a DRY_RUN=false sitting in that profile.
#
#   2. Credentials are injected here rather than left to the binary. Its
#      load_env_files() swallows the result (`let _ = dotenvy::from_path(..)`),
#      so a profile that fails to load reports nothing -- it surfaces later as
#      "missing WALLET_PRIVATE_KEY" warnings and an agent that cannot sign.
#
#   3. It falls back to ~/.meridian/.env when backend/.env is absent. The Rust
#      side resolves its home through HOME, which Windows does not define, so it
#      would otherwise look in "." and find nothing.
#
# Runtime state goes to backend/.meridian so a dry run cannot touch the live
# profile's state file.

$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Backend = Join-Path $Root 'backend'
$Runtime = Join-Path $Backend '.meridian'

New-Item -ItemType Directory -Force -Path $Runtime | Out-Null

$HomeProfile = Join-Path $env:USERPROFILE '.meridian'
$EnvFile = Join-Path $Backend '.env'
if (-not (Test-Path -LiteralPath $EnvFile)) {
  $EnvFile = Join-Path $HomeProfile '.env'
}

if (Test-Path -LiteralPath $EnvFile) {
  Get-Content -LiteralPath $EnvFile | ForEach-Object {
    if ($_ -match '^\s*([^#=][^=]*)=(.*)$') {
      [Environment]::SetEnvironmentVariable($matches[1].Trim(), $matches[2].Trim().Trim('"'), 'Process')
    }
  }
  Write-Host "credentials loaded from $EnvFile"
} else {
  Write-Warning "no .env in $Backend or $HomeProfile; the agent will start without wallet/RPC"
}

$env:MERIDIAN_HOME = Split-Path -Parent $EnvFile

# Safety flag last so nothing loaded above can win over it.
$env:DRY_RUN = 'true'

$env:MERIDIAN_WEB_ADDR = '127.0.0.1:3001'
$env:MERIDIAN_DATA_DIR = $Runtime
$env:MERIDIAN_STATE_PATH = Join-Path $Runtime 'meridian-state.json'
$env:MERIDIAN_LOCK_PATH = Join-Path $Runtime 'meridian.lock'
$env:PATH = 'C:\Strawberry\perl\bin;C:\Strawberry\c\bin;' + $env:PATH

Write-Host "DRY_RUN=$($env:DRY_RUN) HOME=$($env:MERIDIAN_HOME) WEB=$($env:MERIDIAN_WEB_ADDR)"

Set-Location -LiteralPath $Backend
cargo run
