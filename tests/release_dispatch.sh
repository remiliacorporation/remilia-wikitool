#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"

bash scripts/validate_release_dispatch.sh \
  "$version" false refs/heads/codex/local-verification
bash scripts/validate_release_dispatch.sh \
  "$version" true refs/heads/master

if error="$(bash scripts/validate_release_dispatch.sh \
    "${version}-mismatch" false refs/heads/master 2>&1)"; then
  echo "release dispatch validation accepted a mismatched artifact version" >&2
  exit 1
fi
case "$error" in
  *"dispatch the workflow with artifact_version=${version}"*) ;;
  *)
    echo "release version refusal did not name the corrective artifact_version" >&2
    exit 1
    ;;
esac

if error="$(bash scripts/validate_release_dispatch.sh \
    "$version" true refs/heads/codex/local-verification 2>&1)"; then
  echo "release dispatch validation accepted publication from a non-master ref" >&2
  exit 1
fi
case "$error" in
  *"dispatch from master or set create_release=false"*) ;;
  *)
    echo "release ref refusal did not name both corrective choices" >&2
    exit 1
    ;;
esac

if bash scripts/validate_release_dispatch.sh \
    "$version" sometimes refs/heads/master >/dev/null 2>&1; then
  echo "release dispatch validation accepted an invalid create-release value" >&2
  exit 1
fi
