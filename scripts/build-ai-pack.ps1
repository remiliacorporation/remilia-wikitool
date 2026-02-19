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

# Default CLAUDE context is the wikitool-local guidance.
Copy-Item -Path (Join-Path $RepoRoot "CLAUDE.md") -Destination (Join-Path $OutputDir "CLAUDE.md")

# Optionally include parent project Claude context (.claude/rules + .claude/skills).
$hostContextIncluded = $false
$hostRoot = $env:WIKITOOL_HOST_PROJECT_ROOT
if ([string]::IsNullOrWhiteSpace($hostRoot)) {
  $candidates = @(
    (Join-Path $RepoRoot "../.."),
    (Join-Path $RepoRoot ".."),
    (Join-Path $RepoRoot "../../..")
  )
  foreach ($candidate in $candidates) {
    $resolved = $null
    try { $resolved = (Resolve-Path $candidate -ErrorAction Stop).Path } catch {}
    if ([string]::IsNullOrWhiteSpace($resolved)) { continue }
    $claude = Join-Path $resolved "CLAUDE.md"
    $rules = Join-Path $resolved ".claude/rules"
    $skills = Join-Path $resolved ".claude/skills"
    if ((Test-Path $claude) -and (Test-Path $rules) -and (Test-Path $skills)) {
      $hostRoot = $resolved
      break
    }
  }
}

if (![string]::IsNullOrWhiteSpace($hostRoot)) {
  $hostResolved = (Resolve-Path $hostRoot).Path
  if ($hostResolved -ne $RepoRoot) {
    $hostClaude = Join-Path $hostResolved "CLAUDE.md"
    $hostRules = Join-Path $hostResolved ".claude/rules"
    $hostSkills = Join-Path $hostResolved ".claude/skills"
    if ((Test-Path $hostClaude) -and (Test-Path $hostRules) -and (Test-Path $hostSkills)) {
      Copy-Item -Path (Join-Path $RepoRoot "CLAUDE.md") -Destination (Join-Path $OutputDir "WIKITOOL_CLAUDE.md") -Force
      Copy-Item -Path $hostClaude -Destination (Join-Path $OutputDir "CLAUDE.md") -Force
      $claudeDest = Join-Path $OutputDir ".claude"
      New-Item -ItemType Directory -Path $claudeDest -Force | Out-Null
      Copy-Item -Path $hostRules -Destination (Join-Path $claudeDest "rules") -Recurse -Force
      Copy-Item -Path $hostSkills -Destination (Join-Path $claudeDest "skills") -Recurse -Force
      $hostContextIncluded = $true
    }
  }
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

$codexSkillsIncluded = $false
$codexSkillsSource = Join-Path $RepoRoot "codex_skills"
if (Test-Path $codexSkillsSource) {
  $codexSkillsDest = Join-Path $OutputDir "codex_skills"
  New-Item -ItemType Directory -Path $codexSkillsDest -Force | Out-Null
  Copy-Item -Path (Join-Path $codexSkillsSource "*") -Destination $codexSkillsDest -Recurse -Force
  $codexSkillsIncluded = $true
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
  host_context_included = $hostContextIncluded
  codex_skills_included = $codexSkillsIncluded
  docs_bundle_included = $docsBundleIncluded
  notes = "AI companion pack for wikitool; content is intentionally shipped outside the binary."
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -Path (Join-Path $OutputDir "manifest.json")

Write-Output "Built AI pack at $OutputDir"
