#!/usr/bin/env bash
set -euo pipefail

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

cd "${WIKITOOL}"
bun run docs:reference
