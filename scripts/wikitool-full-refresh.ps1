$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
  $projectRoot = $root
} else {
  $wikitool = $root
  $projectRoot = Resolve-Path (Join-Path $root "..")
}
$dbPath = Join-Path -Path $projectRoot -ChildPath ".wikitool\\data\\wikitool.db"

Write-Host "This will reset the local wikitool DB and re-download all content/templates."
$confirm = Read-Host "Continue? (y/N)"
if ($confirm -ne "y") {
  Write-Host "Aborted."
  exit 1
}

if (Test-Path $dbPath) {
  Remove-Item $dbPath -Force
}

Set-Location $wikitool
cargo build --package wikitool --release --locked
$wikitoolBin = Join-Path -Path $wikitool -ChildPath "target\\release\\wikitool.exe"
if (-not (Test-Path $wikitoolBin)) {
  throw "Release binary not found at $wikitoolBin"
}

& $wikitoolBin init --project-root $projectRoot --templates
& (Join-Path -Path $root -ChildPath "scripts\\generate-wikitool-reference.ps1")
& $wikitoolBin pull --project-root $projectRoot --full --all
& $wikitoolBin validate --project-root $projectRoot
& $wikitoolBin status --project-root $projectRoot
cargo test --workspace
