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

if ! command -v bun >/dev/null 2>&1; then
  echo "Bun is required. Run scripts/bootstrap-macos.sh or scripts/bootstrap-linux.sh to install it, then re-run this script."
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -d "${ROOT}/custom/wikitool" ]]; then
  WIKITOOL="${ROOT}/custom/wikitool"
else
  WIKITOOL="${ROOT}"
fi

cd "$WIKITOOL"
if [[ "$REBUILD" -eq 1 ]]; then
  bun install --force
else
  bun install
fi

if [[ "$SKIP_SELENE" -eq 0 ]]; then
  if [[ "$REBUILD" -eq 1 ]]; then
    "${ROOT}/scripts/setup-selene.sh" --force
  else
    "${ROOT}/scripts/setup-selene.sh"
  fi
fi

bin="${WIKITOOL}/node_modules/.bin/lighthouse"
if [[ ! -e "$bin" ]]; then
  echo "Lighthouse binary not found. Run bun install in the wikitool directory."
  exit 1
fi

echo "Lighthouse available at $bin"

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
