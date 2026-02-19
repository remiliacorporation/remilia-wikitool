param(
  [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
  $OutputDir = Join-Path $RepoRoot "dist/ai-pack"
}

if (Test-Path $OutputDir) {
  Remove-Item -Recurse -Force $OutputDir
}
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$requiredFiles = @(
  "AGENTS.md",
  "CLAUDE.md",
  "SETUP.md",
  "README.md"
)

foreach ($file in $requiredFiles) {
  $src = Join-Path $RepoRoot $file
  if (!(Test-Path $src)) {
    throw "Missing required AI pack file: $file"
  }
  Copy-Item -Path $src -Destination (Join-Path $OutputDir $file)
}

$llmSource = Join-Path $RepoRoot "llm_instructions"
$llmDest = Join-Path $OutputDir "llm_instructions"
New-Item -ItemType Directory -Path $llmDest -Force | Out-Null
$llmFiles = Get-ChildItem -Path $llmSource -Filter *.md -File -ErrorAction SilentlyContinue
if ($llmFiles.Count -eq 0) {
  throw "No llm_instructions/*.md files found"
}
Copy-Item -Path (Join-Path $llmSource "*.md") -Destination $llmDest

$docsSource = Join-Path $RepoRoot "docs/wikitool"
if (Test-Path $docsSource) {
  $docsDest = Join-Path $OutputDir "docs/wikitool"
  New-Item -ItemType Directory -Path $docsDest -Force | Out-Null
  Get-ChildItem -Path $docsSource -Filter *.md -File | ForEach-Object {
    Copy-Item -Path $_.FullName -Destination (Join-Path $docsDest $_.Name)
  }
}

$docsBundleIncluded = $false
$docsBundleSrc = Join-Path $RepoRoot "ai/docs-bundle-v1.json"
if (Test-Path $docsBundleSrc) {
  $aiDest = Join-Path $OutputDir "ai"
  New-Item -ItemType Directory -Path $aiDest -Force | Out-Null
  Copy-Item -Path $docsBundleSrc -Destination (Join-Path $aiDest "docs-bundle-v1.json")
  $docsBundleIncluded = $true
}

$manifest = [ordered]@{
  schema_version = 1
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  docs_bundle_included = $docsBundleIncluded
  notes = "AI companion pack for wikitool; content is intentionally shipped outside the binary."
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $OutputDir "manifest.json")

Write-Output "Built AI pack at $OutputDir"
