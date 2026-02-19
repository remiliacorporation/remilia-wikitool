$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (Test-Path (Join-Path $root "custom\wikitool")) {
  $wikitool = Join-Path -Path $root -ChildPath "custom\wikitool"
} else {
  $wikitool = $root
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "cargo is required to generate docs/wikitool/reference.md"
}

Set-Location $wikitool

function Get-HelpText {
  param([string[]]$CliArgs)

  $cmdArgs = @("run", "--quiet", "--package", "wikitool", "--") + $CliArgs + @("--help")
  $output = & cargo @cmdArgs 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "Failed to run: cargo $($cmdArgs -join ' ')`n$output"
  }
  return ($output -join "`n").TrimEnd()
}

$sections = @(
  @{ Title = "Global"; Args = @() },
  @{ Title = "init"; Args = @("init") },
  @{ Title = "pull"; Args = @("pull") },
  @{ Title = "push"; Args = @("push") },
  @{ Title = "diff"; Args = @("diff") },
  @{ Title = "status"; Args = @("status") },
  @{ Title = "context"; Args = @("context") },
  @{ Title = "search"; Args = @("search") },
  @{ Title = "search-external"; Args = @("search-external") },
  @{ Title = "validate"; Args = @("validate") },
  @{ Title = "lint"; Args = @("lint") },
  @{ Title = "fetch"; Args = @("fetch") },
  @{ Title = "export"; Args = @("export") },
  @{ Title = "delete"; Args = @("delete") },
  @{ Title = "db"; Args = @("db") },
  @{ Title = "db stats"; Args = @("db", "stats") },
  @{ Title = "db sync"; Args = @("db", "sync") },
  @{ Title = "db migrate"; Args = @("db", "migrate") },
  @{ Title = "docs"; Args = @("docs") },
  @{ Title = "docs import"; Args = @("docs", "import") },
  @{ Title = "docs import-technical"; Args = @("docs", "import-technical") },
  @{ Title = "docs list"; Args = @("docs", "list") },
  @{ Title = "docs update"; Args = @("docs", "update") },
  @{ Title = "docs remove"; Args = @("docs", "remove") },
  @{ Title = "docs search"; Args = @("docs", "search") },
  @{ Title = "seo inspect"; Args = @("seo", "inspect") },
  @{ Title = "net inspect"; Args = @("net", "inspect") },
  @{ Title = "perf lighthouse"; Args = @("perf", "lighthouse") },
  @{ Title = "import cargo"; Args = @("import", "cargo") },
  @{ Title = "index"; Args = @("index") },
  @{ Title = "index rebuild"; Args = @("index", "rebuild") },
  @{ Title = "index stats"; Args = @("index", "stats") },
  @{ Title = "index backlinks"; Args = @("index", "backlinks") },
  @{ Title = "index orphans"; Args = @("index", "orphans") },
  @{ Title = "index prune-categories"; Args = @("index", "prune-categories") },
  @{ Title = "lsp:generate-config"; Args = @("lsp:generate-config") },
  @{ Title = "lsp:status"; Args = @("lsp:status") },
  @{ Title = "lsp:info"; Args = @("lsp:info") },
  @{ Title = "contracts"; Args = @("contracts") },
  @{ Title = "contracts snapshot"; Args = @("contracts", "snapshot") },
  @{ Title = "contracts command-surface"; Args = @("contracts", "command-surface") }
)

$lines = @(
  "# Wikitool Command Reference",
  "",
  "This file is generated from Rust CLI help output. Do not edit manually.",
  "",
  "Regenerate:",
  "",
  '```bash',
  "scripts/generate-wikitool-reference.ps1",
  "scripts/generate-wikitool-reference.sh",
  '```',
  ""
)

foreach ($section in $sections) {
  $help = Get-HelpText -CliArgs $section.Args
  $lines += "## $($section.Title)"
  $lines += ""
  $lines += '```text'
  $lines += $help
  $lines += '```'
  $lines += ""
}

$outputPath = Join-Path $wikitool "docs/wikitool/reference.md"
$dir = Split-Path -Parent $outputPath
if (!(Test-Path $dir)) {
  New-Item -ItemType Directory -Path $dir -Force | Out-Null
}
$lines | Set-Content -Encoding utf8 $outputPath
Write-Output "Wrote $outputPath"
