$ErrorActionPreference = "Stop"

if (-not (Get-Command bun -ErrorAction SilentlyContinue)) {
  Write-Error "Bun is required. Run scripts/bootstrap-windows.ps1 to install it, then re-run this script."
  exit 1
}

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
} else {
  $wikitool = $root
}

Set-Location $wikitool
bun run docs:reference
