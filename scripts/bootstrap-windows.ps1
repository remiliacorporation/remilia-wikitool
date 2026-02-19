param(
  [switch]$Rebuild,
  [switch]$Fix,
  [switch]$Pull,
  [switch]$NoPull,
  [switch]$SkipSelene
)

$ErrorActionPreference = "Stop"

if ($Fix) {
  $Rebuild = $true
}

if (-not $SkipSelene -and $env:WIKITOOL_SKIP_SELENE -eq "1") {
  $SkipSelene = $true
}

if ($Pull -and $NoPull) {
  Write-Error "Use -Pull or -NoPull, not both."
  exit 1
}

$pullContent = -not $NoPull
if ($Pull) {
  $pullContent = $true
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Warning "cargo not found in PATH. Install Rust (https://rustup.rs/) and re-run this script."
  exit 1
}

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
  $projectRoot = $root
  $wikitoolLabel = "custom\\wikitool"
} else {
  $wikitool = $root
  $projectRoot = Resolve-Path (Join-Path $root "..")
  $wikitoolLabel = "."
}

$setupArgs = @{
  Rebuild = $Rebuild
  SkipSelene = $SkipSelene
}
& (Join-Path -Path $root -ChildPath "scripts\\setup-tools.ps1") @setupArgs

Set-Location $wikitool
cargo build --package wikitool --release --locked
$wikitoolBin = Join-Path -Path $wikitool -ChildPath "target\\release\\wikitool.exe"
if (-not (Test-Path $wikitoolBin)) {
  Write-Error "Release binary not found at $wikitoolBin"
  exit 1
}

& $wikitoolBin init --project-root $projectRoot --templates
& (Join-Path -Path $root -ChildPath "scripts\\generate-wikitool-reference.ps1")

& (Join-Path -Path $root -ChildPath "scripts\\install-git-hooks.ps1")

if ($pullContent) {
  Write-Host ""
  Write-Host "Pulling wiki content..." -ForegroundColor Cyan
  & $wikitoolBin pull --project-root $projectRoot --full --all
  Write-Host "Content pulled successfully." -ForegroundColor Green
} else {
  Write-Host ""
  Write-Host "Bootstrap complete. Content pull skipped." -ForegroundColor Yellow
  Write-Host "Next step: $wikitoolBin --project-root $projectRoot pull" -ForegroundColor Yellow
  Write-Host "Re-run without -NoPull (or with -Pull) to auto-download content." -ForegroundColor Yellow
}
