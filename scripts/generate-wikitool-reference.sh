#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -d "${ROOT}/custom/wikitool" ]]; then
  WIKITOOL="${ROOT}/custom/wikitool"
else
  WIKITOOL="${ROOT}"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to generate docs/wikitool/reference.md" >&2
  exit 1
fi

cd "${WIKITOOL}"

help_text() {
  cargo run --quiet --package wikitool -- "$@" --help
}

SECTIONS=(
  "Global|"
  "init|init"
  "pull|pull"
  "push|push"
  "diff|diff"
  "status|status"
  "context|context"
  "search|search"
  "search-external|search-external"
  "validate|validate"
  "lint|lint"
  "fetch|fetch"
  "export|export"
  "delete|delete"
  "db|db"
  "db stats|db stats"
  "db sync|db sync"
  "db migrate|db migrate"
  "docs|docs"
  "docs import|docs import"
  "docs import-technical|docs import-technical"
  "docs list|docs list"
  "docs update|docs update"
  "docs remove|docs remove"
  "docs search|docs search"
  "seo inspect|seo inspect"
  "net inspect|net inspect"
  "perf lighthouse|perf lighthouse"
  "import cargo|import cargo"
  "index|index"
  "index rebuild|index rebuild"
  "index stats|index stats"
  "index backlinks|index backlinks"
  "index orphans|index orphans"
  "index prune-categories|index prune-categories"
  "lsp:generate-config|lsp:generate-config"
  "lsp:status|lsp:status"
  "lsp:info|lsp:info"
  "contracts|contracts"
  "contracts snapshot|contracts snapshot"
  "contracts command-surface|contracts command-surface"
)

OUT="${WIKITOOL}/docs/wikitool/reference.md"
mkdir -p "$(dirname "${OUT}")"

{
  echo "# Wikitool Command Reference"
  echo
  echo "This file is generated from Rust CLI help output. Do not edit manually."
  echo
  echo "Regenerate:"
  echo
  echo '```bash'
  echo "scripts/generate-wikitool-reference.ps1"
  echo "scripts/generate-wikitool-reference.sh"
  echo '```'
  echo

  for section in "${SECTIONS[@]}"; do
    title="${section%%|*}"
    args="${section#*|}"

    echo "## ${title}"
    echo
    echo '```text'
    if [[ -n "${args}" ]]; then
      # shellcheck disable=SC2086
      help_text ${args}
    else
      help_text
    fi
    echo '```'
    echo
  done
} > "${OUT}"

echo "Wrote ${OUT}"
