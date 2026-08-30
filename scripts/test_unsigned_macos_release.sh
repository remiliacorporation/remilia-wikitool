#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

bundle="$fixture/wikitool-test-macos-arm64"
mkdir -p \
  "$bundle/contextmink" \
  "$bundle/papertiger" \
  "$bundle/docs/wikitool" \
  "$bundle/skills/wikitool"
printf 'fixture\n' > "$bundle/wikitool"
printf 'fixture\n' > "$bundle/contextmink/contextmink"
printf 'fixture\n' > "$bundle/papertiger/papertiger"
printf 'fixture\n' > "$bundle/papertiger/papertiger-mise"
cp "$repo_root/docs/wikitool/macos-gatekeeper.md" "$bundle/docs/wikitool/macos-gatekeeper.md"
cp "$repo_root/.agents/skills/wikitool/SKILL.md" \
  "$bundle/skills/wikitool/SKILL.md"

bash "$repo_root/scripts/declare_unsigned_macos.sh" --bundle-dir "$bundle" >/dev/null

trust="$bundle/macos-release-trust.json"
grep -q '"schema": "wikitool.macos-release-trust.v1"' "$trust"
grep -q '"status": "unsigned_github_release"' "$trust"
grep -q '"gatekeeper": "explicit_checksum_bound_quarantine_exception_required"' "$trust"
grep -q '"executables": \["wikitool", "contextmink/contextmink", "papertiger/papertiger", "papertiger/papertiger-mise"\]' "$trust"
grep -q '"instructions": "docs/wikitool/macos-gatekeeper.md"' "$trust"

if bash "$repo_root/scripts/declare_unsigned_macos.sh" --bundle-dir "$bundle" >/dev/null 2>&1; then
  echo "unsigned trust declaration unexpectedly replaced an existing declaration" >&2
  exit 1
fi

rm "$bundle/papertiger/papertiger"
rm "$trust"
if bash "$repo_root/scripts/declare_unsigned_macos.sh" --bundle-dir "$bundle" >/dev/null 2>&1; then
  echo "unsigned trust declaration accepted a bundle without Papertiger" >&2
  exit 1
fi

if grep -R -q -- 'xattr\|-dr\|spctl --master-disable' \
  "$repo_root/scripts/declare_unsigned_macos.sh"; then
  echo "unsigned trust declaration script contains a Gatekeeper mutation" >&2
  exit 1
fi

echo "unsigned macOS release declaration test passed"
