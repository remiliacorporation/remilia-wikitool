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

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
} else {
  $wikitool = $root
}

$seleneArgs = @()
if (-not $SkipSelene) {
  if ($Rebuild) {
    $seleneArgs += "-Force"
  }
  & (Join-Path -Path $root -ChildPath "scripts\\setup-selene.ps1") @seleneArgs
}

$candidates = @()
try {
  $resolved = Get-Command lighthouse -ErrorAction Stop
  if ($resolved.Source) {
    $candidates += $resolved.Source
  }
} catch {}

$binDir = Join-Path -Path $wikitool -ChildPath "node_modules\.bin"
$candidates += @(
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
  Write-Warning "Lighthouse not found on PATH or node_modules/.bin. perf lighthouse will remain unavailable until installed."
} else {
  Write-Host "Lighthouse available at $found"
}

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
