#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: sign_notarize_macos.sh \
  --bundle-dir DIR \
  --zip-path FILE \
  --identity DEVELOPER_IDENTITY \
  --identifier-prefix REVERSE_DNS_PREFIX \
  --notary-key FILE \
  --notary-key-id ID \
  --notary-issuer ID \
  --evidence-dir DIR

Signs every Mach-O file in a staged Wikitool release bundle, rebuilds the ZIP,
submits that exact ZIP to Apple's notary service, retains the submission and
log evidence, and checks each executable with Gatekeeper's assessment tool.
EOF
}

bundle_dir=""
zip_path=""
identity=""
identifier_prefix=""
notary_key=""
notary_key_id=""
notary_issuer=""
evidence_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle-dir)
      bundle_dir="${2:-}"
      shift 2
      ;;
    --zip-path)
      zip_path="${2:-}"
      shift 2
      ;;
    --identity)
      identity="${2:-}"
      shift 2
      ;;
    --identifier-prefix)
      identifier_prefix="${2:-}"
      shift 2
      ;;
    --notary-key)
      notary_key="${2:-}"
      shift 2
      ;;
    --notary-key-id)
      notary_key_id="${2:-}"
      shift 2
      ;;
    --notary-issuer)
      notary_issuer="${2:-}"
      shift 2
      ;;
    --evidence-dir)
      evidence_dir="${2:-}"
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

for value_name in bundle_dir zip_path identity identifier_prefix notary_key notary_key_id notary_issuer evidence_dir; do
  if [[ -z "${!value_name}" ]]; then
    echo "missing required argument: ${value_name}" >&2
    usage >&2
    exit 2
  fi
done

if [[ ! "$identifier_prefix" =~ ^[A-Za-z0-9.-]+$ ]]; then
  echo "invalid code-signing identifier prefix: $identifier_prefix" >&2
  exit 2
fi

if [[ ! -d "$bundle_dir" ]]; then
  echo "bundle directory does not exist: $bundle_dir" >&2
  exit 1
fi
if [[ ! -f "$notary_key" ]]; then
  echo "notary API key does not exist: $notary_key" >&2
  exit 1
fi
if [[ ! -f "$bundle_dir/wikitool" ]]; then
  echo "staged macOS bundle is missing wikitool: $bundle_dir" >&2
  exit 1
fi
if [[ ! -f "$bundle_dir/contextmink/contextmink" ]]; then
  echo "staged macOS bundle is missing contextmink: $bundle_dir" >&2
  exit 1
fi

for required_tool in codesign ditto file find grep mkdir plutil rm spctl tr xcrun; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    echo "required macOS release tool is unavailable: $required_tool" >&2
    exit 1
  fi
done

mkdir -p "$evidence_dir"
codesign_evidence="$evidence_dir/codesign.txt"
assessment_evidence="$evidence_dir/gatekeeper-assessment.txt"
submission_evidence="$evidence_dir/notary-submission.json"
notary_log="$evidence_dir/notary-log.json"
: > "$codesign_evidence"
: > "$assessment_evidence"

mach_o_files=()
while IFS= read -r -d '' candidate; do
  if file -b "$candidate" | grep -q 'Mach-O'; then
    mach_o_files+=("$candidate")
  fi
done < <(find "$bundle_dir" -type f -print0)

if [[ ${#mach_o_files[@]} -eq 0 ]]; then
  echo "release bundle contains no Mach-O files: $bundle_dir" >&2
  exit 1
fi

contains_path() {
  local expected="$1"
  local candidate
  for candidate in "${mach_o_files[@]}"; do
    if [[ "$candidate" == "$expected" ]]; then
      return 0
    fi
  done
  return 1
}

contains_path "$bundle_dir/wikitool" || {
  echo "wikitool is not a Mach-O executable: $bundle_dir/wikitool" >&2
  exit 1
}
contains_path "$bundle_dir/contextmink/contextmink" || {
  echo "contextmink is not a Mach-O executable: $bundle_dir/contextmink/contextmink" >&2
  exit 1
}

for executable in "${mach_o_files[@]}"; do
  relative_path="${executable#"$bundle_dir"/}"
  identifier_suffix="$(printf '%s' "$relative_path" | tr '/_ ' '...')"
  identifier="$identifier_prefix.$identifier_suffix"
  printf 'sign %s identifier=%s\n' "$relative_path" "$identifier" >> "$codesign_evidence"
  codesign --force --identifier "$identifier" --options runtime --timestamp --sign "$identity" "$executable"
  codesign --verify --strict --verbose=4 "$executable" 2>> "$codesign_evidence"
  codesign --display --verbose=4 "$executable" 2>> "$codesign_evidence"
done

mkdir -p "$(dirname "$zip_path")"
rm -f "$zip_path"
ditto -c -k --sequesterRsrc --keepParent "$bundle_dir" "$zip_path"

xcrun notarytool submit "$zip_path" \
  --key "$notary_key" \
  --key-id "$notary_key_id" \
  --issuer "$notary_issuer" \
  --wait \
  --output-format json > "$submission_evidence"

submission_status="$(plutil -extract status raw -o - "$submission_evidence")"
submission_id="$(plutil -extract id raw -o - "$submission_evidence")"

xcrun notarytool log "$submission_id" \
  --key "$notary_key" \
  --key-id "$notary_key_id" \
  --issuer "$notary_issuer" \
  "$notary_log"

if [[ "$submission_status" != "Accepted" ]]; then
  echo "Apple notarization was not accepted; status=$submission_status id=$submission_id" >&2
  exit 1
fi

for executable in "${mach_o_files[@]}"; do
  printf 'assess %s\n' "${executable#"$bundle_dir"/}" >> "$assessment_evidence"
  spctl --assess --type execute --verbose=4 "$executable" 2>> "$assessment_evidence"
done

echo "signed_mach_o_count=${#mach_o_files[@]}"
echo "notary_submission_id=$submission_id"
echo "notary_status=$submission_status"
echo "zip_path=$zip_path"
echo "evidence_dir=$evidence_dir"
