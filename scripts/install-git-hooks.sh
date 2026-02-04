#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOOKS_DIR="${ROOT}/.git/hooks"

if [ ! -d "${HOOKS_DIR}" ]; then
  echo "No .git/hooks directory found. Git hooks not installed (OK for zip downloads)."
  exit 0
fi

install -m 0755 "${ROOT}/scripts/git-hooks/commit-msg" "${HOOKS_DIR}/commit-msg"
echo "Installed commit-msg hook."
