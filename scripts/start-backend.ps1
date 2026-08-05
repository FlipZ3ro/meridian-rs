$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Backend = Join-Path $Root 'backend'
$Runtime = Join-Path $Backend '.meridian'

New-Item -ItemType Directory -Force -Path $Runtime | Out-Null

# Inject backend/.env into the process environment explicitly. The binary also
# calls dotenvy, but dotenvy does not override already-set vars and was loading
# unreliably here (LLM_API_KEY / RPC_URL showed up missing at startup), so set
# them ourselves to guarantee the runtime has wallet, RPC, and LLM credentials.
$EnvFile = Join-Path $Backend '.env'
if (Test-Path -LiteralPath $EnvFile) {
  Get-Content -LiteralPath $EnvFile | ForEach-Object {
    if ($_ -match '^\s*([^#=][^=]*)=(.*)$') {
      $k = $matches[1].Trim()
      $v = $matches[2].Trim().Trim('"')
      [Environment]::SetEnvironmentVariable($k, $v, 'Process')
    }
  }
}

$env:MERIDIAN_WEB_ADDR = '127.0.0.1:3001'
$env:MERIDIAN_DATA_DIR = $Runtime
$env:MERIDIAN_STATE_PATH = Join-Path $Runtime 'meridian-state.json'
$env:MERIDIAN_LOCK_PATH = Join-Path $Runtime 'meridian.lock'
$env:PATH = 'C:\Strawberry\perl\bin;C:\Strawberry\c\bin;' + $env:PATH

Set-Location -LiteralPath $Backend
cargo run
