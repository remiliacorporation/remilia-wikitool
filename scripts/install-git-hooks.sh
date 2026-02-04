#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOOKS_DIR="${ROOT}/.git/hooks"

if [ ! -d "${HOOKS_DIR}" ]; then
  echo "No .git/hooks directory found. Are you running this inside the repo?"
  exit 1
fi

install -m 0755 "${ROOT}/scripts/git-hooks/commit-msg" "${HOOKS_DIR}/commit-msg"
echo "Installed commit-msg hook."
