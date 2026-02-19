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

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found in PATH. Install Rust (https://rustup.rs/) and re-run."
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
cargo build --package wikitool --release --locked
WIKITOOL_BIN="${WIKITOOL}/target/release/wikitool"
if [[ ! -x "${WIKITOOL_BIN}" ]]; then
  echo "Release binary not found at ${WIKITOOL_BIN}"
  exit 1
fi

"${WIKITOOL_BIN}" init --project-root "${PROJECT_ROOT}" --templates
"${ROOT}/scripts/generate-wikitool-reference.sh"

"${ROOT}/scripts/install-git-hooks.sh"

if [[ "$PULL" -eq 1 ]]; then
  echo ""
  echo "Pulling wiki content..."
  "${WIKITOOL_BIN}" pull --project-root "${PROJECT_ROOT}" --full --all
  echo "Content pulled successfully."
else
  echo ""
  echo "Bootstrap complete. Content pull skipped."
  echo "Next step: ${WIKITOOL_BIN} --project-root ${PROJECT_ROOT} pull"
  echo "Re-run without --no-pull (or with --pull) to auto-download content."
fi
