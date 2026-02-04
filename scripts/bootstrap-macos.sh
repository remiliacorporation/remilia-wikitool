#!/usr/bin/env bash
set -euo pipefail

REBUILD=0
PULL=1
SKIP_SELENE=0
for arg in "$@"; do
  case "$arg" in
    --rebuild|--fix)
      REBUILD=1
      ;;
    --pull)
      PULL=1
      ;;
    --no-pull)
      PULL=0
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

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -d "${ROOT}/custom/wikitool" ]]; then
  WIKITOOL="${ROOT}/custom/wikitool"
  PROJECT_ROOT="${ROOT}"
  WIKITOOL_LABEL="custom/wikitool"
else
  WIKITOOL="${ROOT}"
  PROJECT_ROOT="$(cd "${ROOT}/.." && pwd)"
  WIKITOOL_LABEL="."
fi

if ! command -v bun >/dev/null 2>&1; then
  curl -fsSL https://bun.sh/install | bash
fi

if ! command -v bun >/dev/null 2>&1; then
  echo "Bun not found in PATH. Restart your terminal and re-run this script."
  exit 1
fi

setup_args=()
if [[ "$REBUILD" -eq 1 ]]; then
  setup_args+=("--rebuild")
fi
if [[ "$SKIP_SELENE" -eq 1 ]]; then
  setup_args+=("--skip-selene")
fi
"${ROOT}/scripts/setup-tools.sh" "${setup_args[@]}"

cd "${WIKITOOL}"
bun run build
bun run wikitool init
"${ROOT}/scripts/generate-wikitool-reference.sh"

"${ROOT}/scripts/install-git-hooks.sh"

if [[ "$PULL" -eq 1 ]]; then
  echo ""
  echo "Pulling wiki content..."
  bun run wikitool pull --full --all
  echo "Content pulled successfully."
else
  echo ""
  echo "Bootstrap complete. Content pull skipped."
  echo "Next step: cd ${WIKITOOL_LABEL} && bun run wikitool pull"
  echo "Re-run without --no-pull (or with --pull) to auto-download content."
fi
