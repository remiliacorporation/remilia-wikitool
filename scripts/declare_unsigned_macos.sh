#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: declare_unsigned_macos.sh --bundle-dir DIR

Validate one staged macOS release bundle and add its explicit unsigned GitHub
trust declaration. Packaging and external checksum generation happen after
this script succeeds.
EOF
}

bundle_dir=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle-dir)
      bundle_dir="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$bundle_dir" || ! -d "$bundle_dir" ]]; then
  echo "--bundle-dir must name one staged macOS release directory" >&2
  exit 2
fi

for required in \
  "$bundle_dir/wikitool" \
  "$bundle_dir/contextmink/contextmink" \
  "$bundle_dir/docs/wikitool/macos-gatekeeper.md" \
  "$bundle_dir/codex_skills/wikitool-operator/SKILL.md"
do
  if [[ ! -f "$required" || -L "$required" ]]; then
    echo "unsigned macOS release prerequisite is missing or a symlink: $required" >&2
    exit 1
  fi
done

trust_path="$bundle_dir/macos-release-trust.json"
if [[ -e "$trust_path" || -L "$trust_path" ]]; then
  echo "macOS trust declaration already exists: $trust_path" >&2
  exit 1
fi

printf '%s\n' \
  '{' \
  '  "schema": "wikitool.macos-release-trust.v1",' \
  '  "status": "unsigned_github_release",' \
  '  "gatekeeper": "explicit_checksum_bound_quarantine_exception_required",' \
  '  "instructions": "docs/wikitool/macos-gatekeeper.md"' \
  '}' > "$trust_path"

echo "unsigned_macos_trust=$trust_path"
