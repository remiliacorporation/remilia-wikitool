param(
  [switch]$Rebuild,
  [switch]$Fix,
  [switch]$SkipSelene
)

$ErrorActionPreference = "Stop"

if ($Fix) {
  $Rebuild = $true
}

if (-not $SkipSelene -and $env:WIKITOOL_SKIP_SELENE -eq "1") {
  $SkipSelene = $true
}

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

Push-Location $wikitool
try {
  if ($Rebuild) {
    bun install --force
  } else {
    bun install
  }
} finally {
  Pop-Location
}

$seleneArgs = @()
if (-not $SkipSelene) {
  if ($Rebuild) {
    $seleneArgs += "-Force"
  }
  & (Join-Path -Path $root -ChildPath "scripts\\setup-selene.ps1") @seleneArgs
}

$binDir = Join-Path -Path $wikitool -ChildPath "node_modules\.bin"
$candidates = @(
  (Join-Path $binDir "lighthouse.cmd"),
  (Join-Path $binDir "lighthouse.exe"),
  (Join-Path $binDir "lighthouse")
)

$found = $null
foreach ($candidate in $candidates) {
  if (Test-Path $candidate) {
    $found = $candidate
    break
  }
}

if (-not $found) {
  Write-Error "Lighthouse binary not found. Run bun install in the wikitool directory."
  exit 1
}

Write-Host "Lighthouse available at $found"

if (-not $SkipSelene) {
  $selenePath = Join-Path -Path $wikitool -ChildPath "tools\selene.exe"
  if (-not (Test-Path $selenePath)) {
    Write-Error "Selene binary not found at $selenePath"
    exit 1
  }
  Write-Host "Selene available at $selenePath"
} else {
  Write-Host "Skipping Selene install (Lua linting disabled)."
}
