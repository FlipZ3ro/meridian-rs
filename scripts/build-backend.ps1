$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$Backend = Join-Path $Root 'backend'

$env:PATH = 'C:\Strawberry\perl\bin;C:\Strawberry\c\bin;' + $env:PATH
Set-Location -LiteralPath $Backend
cargo build
