param(
  [string]$Version = $env:SELENE_VERSION,
  [string]$Url = $env:SELENE_URL,
  [string]$Sha256 = $env:SELENE_SHA256,
  [switch]$Force,
  [switch]$Rebuild,
  [switch]$Fix
)

$ErrorActionPreference = "Stop"

if ($Rebuild -or $Fix) {
  $Force = $true
}

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\\wikitool"
} else {
  $wikitool = $root
}
$toolsDir = Join-Path -Path $wikitool -ChildPath "tools"
New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null

$binaryName = "selene.exe"
$binaryPath = Join-Path $toolsDir $binaryName

if ((Test-Path $binaryPath) -and -not $Force) {
  Write-Host "Selene already installed at $binaryPath"
  exit 0
}
if ($Force -and (Test-Path $binaryPath)) {
  Remove-Item -Force $binaryPath
}

if (-not $Url) {
  # New naming convention: selene-VERSION-windows.zip or selene-windows.zip for latest
  if (-not $Version) {
    # Fetch latest version from GitHub API
    $releaseInfo = Invoke-RestMethod -Uri "https://api.github.com/repos/Kampfkarren/selene/releases/latest"
    $Version = $releaseInfo.tag_name
  }
  $asset = "selene-$Version-windows.zip"
  $Url = "https://github.com/Kampfkarren/selene/releases/download/$Version/$asset"
}

$zipPath = Join-Path $toolsDir "selene.zip"
Write-Host "Downloading Selene from $Url..."
Invoke-WebRequest -Uri $Url -OutFile $zipPath

if ($Sha256) {
  $hash = (Get-FileHash -Algorithm SHA256 -Path $zipPath).Hash.ToLower()
  if ($hash -ne $Sha256.ToLower()) {
    Write-Error "Checksum mismatch. Expected $Sha256, got $hash."
    exit 1
  }
}

Write-Host "Extracting..."
Expand-Archive -Path $zipPath -DestinationPath $toolsDir -Force
Remove-Item $zipPath -Force

$exe = Get-ChildItem -Path $toolsDir -Filter "selene*.exe" -Recurse | Select-Object -First 1

if (-not $exe) {
  Write-Error "Selene binary not found after extraction."
  exit 1
}

if ($exe.FullName -ne (Join-Path $toolsDir $binaryName)) {
  Move-Item -Path $exe.FullName -Destination (Join-Path $toolsDir $binaryName) -Force
}
Write-Host "Selene installed to $toolsDir\$binaryName"
