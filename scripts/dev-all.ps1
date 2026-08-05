$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$BackendScript = Join-Path $PSScriptRoot 'start-backend.ps1'
$BackendLog = Join-Path $Root 'backend\backend.log'
$BackendErr = Join-Path $Root 'backend\backend.err.log'

Get-Process -Name 'meridian-rs' -ErrorAction SilentlyContinue | Stop-Process -Force
$LockPath = Join-Path $Root 'backend\.meridian\meridian.lock'
if (Test-Path -LiteralPath $LockPath) {
  Remove-Item -LiteralPath $LockPath -Force
}

Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $BackendScript) -WorkingDirectory $Root -RedirectStandardOutput $BackendLog -RedirectStandardError $BackendErr -WindowStyle Hidden

Set-Location -LiteralPath $Root
# Run the frontend-only script (NOT `npm run dev`, which now points back to this
# script and would recurse infinitely).
npm run dev:web
