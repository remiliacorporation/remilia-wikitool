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
DB_PATH="${WIKITOOL}/data/wikitool.db"
REPORT_DIR="${PROJECT_ROOT}/wikitool_exports"
REPORT_PATH="${REPORT_DIR}/validation-report.md"

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
bun run build
bun run wikitool init
"${ROOT}/scripts/generate-wikitool-reference.sh"
bun run wikitool pull --full --all
mkdir -p "${REPORT_DIR}"
bun run wikitool validate --report "${REPORT_PATH}" --format md --include-remote --remote-limit 200
bun run wikitool status
bun test tests

