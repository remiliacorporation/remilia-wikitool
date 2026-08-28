#!/usr/bin/env bash
set -euo pipefail

# Download a pinned upstream Papertiger release, verify it against
# repository-owned hashes, and stage its complete release pack. Papertiger
# retains its own setup, upgrade, authority, and uninstall lifecycle.

dest="dist/papertiger-dist"
platform=""
fetch_all=0
temp_to_cleanup=""

readonly supported_platforms=(
  "windows-x86_64"
  "linux-x86_64"
  "macos-x86_64"
  "macos-arm64"
)

cleanup() {
  if [[ -n "${temp_to_cleanup:-}" && -d "$temp_to_cleanup" ]]; then
    rm -rf "$temp_to_cleanup"
  fi
}
trap cleanup EXIT

host_platform() {
  case "$(uname -s 2>/dev/null):$(uname -m 2>/dev/null)" in
    Darwin:arm64 | Darwin:aarch64) echo "macos-arm64" ;;
    Darwin:*) echo "macos-x86_64" ;;
    Linux:*) echo "linux-x86_64" ;;
    MINGW*:* | MSYS*:* | CYGWIN*:*) echo "windows-x86_64" ;;
    *)
      echo "fetch_papertiger: cannot infer the host platform; pass --platform" >&2
      exit 64
      ;;
  esac
}

validate_platform() {
  local candidate="$1"
  local supported
  for supported in "${supported_platforms[@]}"; do
    if [[ "$candidate" == "$supported" ]]; then
      return 0
    fi
  done
  echo "fetch_papertiger: unsupported platform: $candidate" >&2
  echo "expected one of: ${supported_platforms[*]}" >&2
  exit 64
}

archive_name() {
  local version="$1"
  local selected_platform="$2"
  case "$selected_platform" in
    windows-x86_64) printf 'papertiger-%s-%s.zip\n' "$version" "$selected_platform" ;;
    *) printf 'papertiger-%s-%s.tar.gz\n' "$version" "$selected_platform" ;;
  esac
}

sha256_of() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print tolower($1)}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print tolower($1)}'
  else
    echo "fetch_papertiger: sha256sum or shasum is required" >&2
    exit 69
  fi
}

stage_platform() {
  local version="$1"
  local selected_platform="$2"
  local hashes_file="$3"
  local archive
  archive="$(archive_name "$version" "$selected_platform")"
  local expected
  expected="$(awk -v name="$archive" '$2 == name { print tolower($1) }' "$hashes_file")"
  if [[ -z "$expected" ]]; then
    echo "fetch_papertiger: no pinned SHA-256 for $archive in $hashes_file" >&2
    exit 65
  fi

  local temp_root
  temp_root="$(mktemp -d)"
  temp_to_cleanup="$temp_root"
  local archive_path="$temp_root/$archive"
  local url="https://github.com/virtualonno/papertiger/releases/download/v${version}/${archive}"
  echo "fetch_papertiger: downloading $url"
  curl --fail --location --proto '=https' --tlsv1.2 --output "$archive_path" "$url"

  local actual
  actual="$(sha256_of "$archive_path")"
  if [[ "$actual" != "$expected" ]]; then
    echo "fetch_papertiger: SHA-256 mismatch for $archive" >&2
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 65
  fi

  local extract_root="$temp_root/extracted"
  mkdir -p "$extract_root"
  case "$archive" in
    *.zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -q "$archive_path" -d "$extract_root"
      elif tar -tf "$archive_path" >/dev/null 2>&1; then
        tar -xf "$archive_path" -C "$extract_root"
      elif command -v 7z >/dev/null 2>&1; then
        7z x -y -o"$extract_root" "$archive_path" >/dev/null
      elif command -v powershell.exe >/dev/null 2>&1 && command -v cygpath >/dev/null 2>&1; then
        local archive_windows
        local extract_windows
        archive_windows="$(cygpath -w "$archive_path")"
        extract_windows="$(cygpath -w "$extract_root")"
        MSYS2_ARG_CONV_EXCL='*' powershell.exe -NoProfile -NonInteractive -Command \
          '& { param([string]$archive, [string]$destination) Expand-Archive -LiteralPath $archive -DestinationPath $destination -Force }' \
          "$archive_windows" "$extract_windows"
      else
        echo "fetch_papertiger: unzip, 7z, a zip-capable tar, or Windows PowerShell is required" >&2
        exit 69
      fi
      ;;
    *.tar.gz) tar -xzf "$archive_path" -C "$extract_root" ;;
    *)
      echo "fetch_papertiger: unsupported archive format: $archive" >&2
      exit 65
      ;;
  esac

  local source_root="$extract_root/papertiger-${version}-${selected_platform}"
  local manifest="$source_root/manifest.json"
  if [[ ! -f "$manifest" ]]; then
    echo "fetch_papertiger: release archive lacks expected manifest: $manifest" >&2
    exit 65
  fi
  if ! grep -Eq '"schema"[[:space:]]*:[[:space:]]*"papertiger.release-manifest.v1"' "$manifest" \
    || ! grep -Eq '"name"[[:space:]]*:[[:space:]]*"papertiger"' "$manifest" \
    || ! grep -Eq '"version"[[:space:]]*:[[:space:]]*"'"$version"'"' "$manifest" \
    || ! grep -Eq '"platform"[[:space:]]*:[[:space:]]*"'"$selected_platform"'"' "$manifest" \
    || ! grep -Eq '"archive"[[:space:]]*:[[:space:]]*"'"$archive"'"' "$manifest"; then
    echo "fetch_papertiger: release manifest does not match ${version}/${selected_platform}/${archive}" >&2
    exit 65
  fi

  local planner="papertiger"
  local mise="papertiger-mise"
  if [[ "$selected_platform" == "windows-x86_64" ]]; then
    planner="papertiger.exe"
    mise="papertiger-mise.exe"
  fi
  for required in \
    "$planner" \
    "$mise" \
    agent_integration.md \
    README.md \
    CHANGELOG.md \
    LICENSE \
    manifest.json
  do
    if [[ ! -f "$source_root/$required" || -L "$source_root/$required" ]]; then
      echo "fetch_papertiger: required release file is missing or a symlink: $source_root/$required" >&2
      exit 65
    fi
  done

  local out="$dest/$selected_platform"
  rm -rf "$out"
  mkdir -p "$out"
  cp -R "$source_root/." "$out/"
  printf '%s  %s\n' "$expected" "$archive" > "$out/archive.sha256"
  echo "papertiger ${version} (${selected_platform}) -> $out"
  rm -rf "$temp_root"
  temp_to_cleanup=""
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)
      platform="${2:?--platform requires a value}"
      shift 2
      ;;
    --all)
      fetch_all=1
      shift
      ;;
    --dest)
      dest="${2:?--dest requires a value}"
      shift 2
      ;;
    --help | -h)
      echo "usage: fetch_papertiger.sh [--platform <platform> | --all] [--dest <dir>]"
      echo "  Downloads the version pinned by config/papertiger.version and verifies"
      echo "  it against config/papertiger-sha256s.txt before staging release files."
      echo "  --platform defaults to the host platform."
      exit 0
      ;;
    *)
      echo "fetch_papertiger: unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [[ "$fetch_all" -eq 1 && -n "$platform" ]]; then
  echo "fetch_papertiger: --all and --platform are mutually exclusive" >&2
  exit 64
fi
if [[ ! -f config/papertiger.version || ! -f config/papertiger-sha256s.txt ]]; then
  echo "fetch_papertiger: run from the wikitool repository root (Papertiger pin files not found)" >&2
  exit 65
fi
pin="$(tr -d ' \t\r\n' < config/papertiger.version)"
if [[ ! "$pin" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "fetch_papertiger: invalid version pin: $pin" >&2
  exit 65
fi

if [[ "$fetch_all" -eq 1 ]]; then
  for selected in "${supported_platforms[@]}"; do
    stage_platform "$pin" "$selected" "config/papertiger-sha256s.txt"
  done
else
  if [[ -z "$platform" ]]; then
    platform="$(host_platform)"
  fi
  validate_platform "$platform"
  stage_platform "$pin" "$platform" "config/papertiger-sha256s.txt"
fi
