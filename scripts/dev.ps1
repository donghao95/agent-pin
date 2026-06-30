[CmdletBinding()]
param(
  [switch]$Install,
  [switch]$SkipChecks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

function Test-Command {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Name
  )

  return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

if (-not $SkipChecks) {
  $missing = @()

  foreach ($command in @("node", "pnpm", "cargo")) {
    if (-not (Test-Command $command)) {
      $missing += $command
    }
  }

  if ($missing.Count -gt 0) {
    Write-Error ("Missing required command(s): {0}" -f ($missing -join ", "))
    exit 1
  }
}

if ($Install -or -not (Test-Path (Join-Path $RepoRoot "node_modules"))) {
  Write-Host "[agent-pin] Installing dependencies..."
  pnpm install
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
}

$env:RUST_BACKTRACE = "1"

Write-Host "[agent-pin] Starting desktop dev server..."
Write-Host "[agent-pin] Frontend: http://127.0.0.1:1800"
Write-Host "[agent-pin] HTTP API: http://127.0.0.1:4317"
Write-Host "[agent-pin] Stop with Ctrl+C."

pnpm dev
exit $LASTEXITCODE
