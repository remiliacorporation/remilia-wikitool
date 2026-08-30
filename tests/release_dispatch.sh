#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"
workflow=.github/workflows/release-artifacts.yml

if ! grep -Fq -- '--target "$GITHUB_SHA"' "$workflow"; then
  echo "release publication does not bind automatic tag creation to the dispatch commit" >&2
  exit 1
fi
validator_calls="$(grep -Fc 'bash scripts/validate_release_dispatch.sh' "$workflow")"
if [[ "$validator_calls" -ne 1 ]]; then
  echo "release dispatch validation must run exactly once before the native build matrix (got ${validator_calls})" >&2
  exit 1
fi
if ! grep -Fq 'needs: preflight' "$workflow" \
    || ! grep -Fq 'needs: [preflight, build]' "$workflow" \
    || ! grep -Fq '${{ needs.preflight.outputs.version }}' "$workflow"; then
  echo "release jobs are not consistently bound to the validated preflight version" >&2
  exit 1
fi

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
