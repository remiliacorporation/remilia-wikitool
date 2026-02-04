$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
  $projectRoot = $root
} else {
  $wikitool = $root
  $projectRoot = Resolve-Path (Join-Path $root "..")
}
$dbPath = Join-Path -Path $wikitool -ChildPath "data\\wikitool.db"
$reportDir = Join-Path -Path $projectRoot -ChildPath "wikitool_exports"
$reportPath = Join-Path -Path $reportDir -ChildPath "validation-report.md"

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
bun run build
bun run wikitool init
& (Join-Path -Path $root -ChildPath "scripts\\generate-wikitool-reference.ps1")
bun run wikitool pull --full --all
if (!(Test-Path $reportDir)) {
  New-Item -ItemType Directory -Path $reportDir | Out-Null
}
bun run wikitool validate --report $reportPath --format md --include-remote --remote-limit 200
bun run wikitool status
bun test tests

