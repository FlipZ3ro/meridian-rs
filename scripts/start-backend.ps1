$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Backend = Join-Path $Root 'backend'
$Runtime = Join-Path $Backend '.meridian'

New-Item -ItemType Directory -Force -Path $Runtime | Out-Null

$env:MERIDIAN_WEB_ADDR = '127.0.0.1:3001'
$env:MERIDIAN_DATA_DIR = $Runtime
$env:MERIDIAN_STATE_PATH = Join-Path $Runtime 'meridian-state.json'
$env:MERIDIAN_LOCK_PATH = Join-Path $Runtime 'meridian.lock'
$env:PATH = 'C:\Strawberry\perl\bin;C:\Strawberry\c\bin;' + $env:PATH

Set-Location -LiteralPath $Backend
cargo run
