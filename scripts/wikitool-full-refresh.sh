#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -d "${ROOT}/custom/wikitool" ]]; then
  WIKITOOL="${ROOT}/custom/wikitool"
  PROJECT_ROOT="${ROOT}"
else
  WIKITOOL="${ROOT}"
  PROJECT_ROOT="$(cd "${ROOT}/.." && pwd)"
fi
DB_PATH="${PROJECT_ROOT}/.wikitool/data/wikitool.db"

echo "This will reset the local wikitool DB and re-download all content/templates."
read -r -p "Continue? (y/N) " confirm
if [ "${confirm}" != "y" ]; then
  echo "Aborted."
  exit 1
fi

if [ -f "${DB_PATH}" ]; then
  rm -f "${DB_PATH}"
fi

cd "${WIKITOOL}"
cargo build --package wikitool --release --locked
WIKITOOL_BIN="${WIKITOOL}/target/release/wikitool"
if [[ ! -x "${WIKITOOL_BIN}" ]]; then
  echo "Release binary not found at ${WIKITOOL_BIN}"
  exit 1
fi

"${WIKITOOL_BIN}" init --project-root "${PROJECT_ROOT}" --templates
"${ROOT}/scripts/generate-wikitool-reference.sh"
"${WIKITOOL_BIN}" pull --project-root "${PROJECT_ROOT}" --full --all
"${WIKITOOL_BIN}" validate --project-root "${PROJECT_ROOT}"
"${WIKITOOL_BIN}" status --project-root "${PROJECT_ROOT}"
cargo test --workspace
