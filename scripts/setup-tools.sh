#!/usr/bin/env bash
set -euo pipefail

REBUILD=0
SKIP_SELENE=0
for arg in "$@"; do
  case "$arg" in
    --rebuild|--fix)
      REBUILD=1
      ;;
    --skip-selene)
      SKIP_SELENE=1
      ;;
    *)
      echo "Unknown option: $arg"
      exit 1
      ;;
  esac
done

if [[ "${WIKITOOL_SKIP_SELENE:-}" == "1" ]]; then
  SKIP_SELENE=1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -d "${ROOT}/custom/wikitool" ]]; then
  WIKITOOL="${ROOT}/custom/wikitool"
else
  WIKITOOL="${ROOT}"
fi

if [[ "$SKIP_SELENE" -eq 0 ]]; then
  if [[ "$REBUILD" -eq 1 ]]; then
    "${ROOT}/scripts/setup-selene.sh" --force
  else
    "${ROOT}/scripts/setup-selene.sh"
  fi
fi

found_lighthouse=""
if command -v lighthouse >/dev/null 2>&1; then
  found_lighthouse="$(command -v lighthouse)"
fi

if [[ -z "$found_lighthouse" ]]; then
  echo "Warning: Lighthouse not found on PATH. perf lighthouse will remain unavailable until installed."
else
  echo "Lighthouse available at $found_lighthouse"
fi

if [[ "$SKIP_SELENE" -eq 0 ]]; then
  # Validate selene installation
  selene_bin=$(find "${WIKITOOL}/tools" -maxdepth 1 -type f -name "selene*" ! -name "*.zip" 2>/dev/null | head -n 1)
  if [[ -z "$selene_bin" || ! -x "$selene_bin" ]]; then
    echo "Selene binary not found in ${WIKITOOL}/tools/"
    exit 1
  fi
  echo "Selene available at $selene_bin"
else
  echo "Skipping Selene install (Lua linting disabled)."
fi
