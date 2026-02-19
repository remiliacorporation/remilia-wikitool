param(
  [string]$BinaryPath = "",
  [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($BinaryPath)) {
  $BinaryPath = Join-Path $RepoRoot "target/release/wikitool.exe"
}
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
  $OutputDir = Join-Path $RepoRoot "dist/release"
}

if (!(Test-Path $BinaryPath)) {
  throw "Missing release binary: $BinaryPath"
}

$aiPackDir = Join-Path $RepoRoot "dist/ai-pack"
& (Join-Path $RepoRoot "scripts/build-ai-pack.ps1") -OutputDir $aiPackDir

if (Test-Path $OutputDir) {
  Remove-Item -Recurse -Force $OutputDir
}
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

Copy-Item -Path $BinaryPath -Destination (Join-Path $OutputDir "wikitool.exe")
Copy-Item -Path (Join-Path $aiPackDir "*") -Destination $OutputDir -Recurse -Force

Write-Output "Packaged release at $OutputDir"
