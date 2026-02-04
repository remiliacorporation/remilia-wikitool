$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$hooksDir = Join-Path -Path $root -ChildPath ".git\\hooks"
$source = Join-Path -Path $root -ChildPath "scripts\\git-hooks\\commit-msg"
$dest = Join-Path -Path $hooksDir -ChildPath "commit-msg"

if (-not (Test-Path $hooksDir)) {
  Write-Error "No .git\\hooks directory found. Are you running this inside the repo?"
  exit 1
}

Copy-Item $source $dest -Force
Write-Host "Installed commit-msg hook."
