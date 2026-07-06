#!/usr/bin/env bash
set -euo pipefail

# Stage the vendored contextmink source into <dest>/<platform>/ using the same
# pack layout shipped inside wikitool release bundles. The release builder does
# this directly in Rust; this helper is for local/manual staging and optional
# repo-local install.

dest="dist/contextmink-dist"
platform=""
target=""
source="vendor/contextmink"
install=0
use_locked=1

host_platform() {
  case "$(uname -s 2>/dev/null):$(uname -m 2>/dev/null)" in
    Darwin:arm64) echo "macos-arm64" ;;
    Darwin:*) echo "macos-x86_64" ;;
    Linux:*) echo "linux-x86_64" ;;
    *) echo "windows-x86_64" ;;
  esac
}

default_target_for_platform() {
  case "$1" in
    windows-x86_64) echo "x86_64-pc-windows-msvc" ;;
    linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    macos-x86_64) echo "x86_64-apple-darwin" ;;
    macos-arm64) echo "aarch64-apple-darwin" ;;
    *)
      echo "fetch_contextmink: unsupported platform: $1" >&2
      echo "expected one of: windows-x86_64 linux-x86_64 macos-x86_64 macos-arm64" >&2
      exit 64
      ;;
  esac
}

contextmink_version_from_pkgid() {
  local pkgid="$1"
  local version="${pkgid##*@}"
  if [[ "$version" == "$pkgid" ]]; then
    version="${pkgid##*#}"
  fi
  printf '%s\n' "$version"
}

binary_names_for_platform() {
  case "$1" in
    windows-x86_64) echo "contextmink.exe contextmink-bridge.exe" ;;
    linux-x86_64 | macos-x86_64 | macos-arm64) echo "contextmink" ;;
    *)
      echo "fetch_contextmink: unsupported platform: $1" >&2
      exit 64
      ;;
  esac
}

install_binary() {
  local source_file="$1"
  local target_file="$2"
  if [[ -f "$target_file" ]] && cmp -s "$source_file" "$target_file"; then
    chmod +x "$target_file"
    return 0
  fi
  if ! cp "$source_file" "$target_file"; then
    echo "fetch_contextmink: failed to install $target_file" >&2
    echo "fetch_contextmink: on Windows this usually means that binary is running; close it or run from Git Bash instead of through contextmink-bridge" >&2
    exit 69
  fi
  chmod +x "$target_file"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)
      platform="${2:?--platform requires a value}"
      shift 2
      ;;
    --target)
      target="${2:?--target requires a Rust target triple}"
      shift 2
      ;;
    --dest)
      dest="${2:?--dest requires a value}"
      shift 2
      ;;
    --source | --local)
      source="${2:?--source requires a contextmink source checkout path}"
      shift 2
      ;;
    --install)
      install=1
      shift
      ;;
    --no-locked)
      use_locked=0
      shift
      ;;
    --repo)
      echo "fetch_contextmink: --repo was removed; contextmink is vendored at vendor/contextmink" >&2
      exit 64
      ;;
    --help | -h)
      echo "usage: fetch_contextmink.sh [--platform <windows-x86_64|linux-x86_64|macos-x86_64|macos-arm64>] [--target <triple>] [--dest <dir>] [--source <checkout>] [--install] [--no-locked]"
      echo "  --platform defaults to the host platform."
      echo "  --target defaults from --platform."
      echo "  --source defaults to vendor/contextmink."
      echo "  --install additionally places binaries in tools/contextmink/bin/ and the"
      echo "  launcher at scripts/contextmink for repo-local agent use."
      exit 0
      ;;
    *)
      echo "fetch_contextmink: unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [[ -z "$platform" ]]; then
  platform="$(host_platform)"
fi
if [[ -z "$target" ]]; then
  target="$(default_target_for_platform "$platform")"
else
  # Validate the platform slug even when an explicit Rust target is supplied.
  default_target_for_platform "$platform" >/dev/null
fi

if [[ ! -f config/contextmink.version ]]; then
  echo "fetch_contextmink: run from the wikitool repository root (config/contextmink.version not found)" >&2
  exit 65
fi
pin="$(tr -d ' \t\r\n' < config/contextmink.version)"
if [[ -z "$pin" ]]; then
  echo "fetch_contextmink: config/contextmink.version is empty" >&2
  exit 65
fi

if [[ ! -f "$source/Cargo.toml" ]] \
  || ! grep -q '^name = "contextmink"' "$source/Cargo.toml"; then
  echo "fetch_contextmink: $source is not a contextmink checkout (no Cargo.toml with name \"contextmink\")" >&2
  exit 65
fi

pkgid="$(cargo pkgid --manifest-path "$source/Cargo.toml")"
version="$(contextmink_version_from_pkgid "$pkgid")"
if [[ "$version" != "$pin" ]]; then
  echo "fetch_contextmink: contextmink source is ${version}, pin is ${pin}" >&2
  exit 65
fi

build_args=(build --release --bins --manifest-path "$source/Cargo.toml" --target "$target")
if [[ "$use_locked" -eq 1 ]]; then
  build_args+=(--locked)
fi
cargo "${build_args[@]}"

out="${dest}/${platform}"
rm -rf "$out"
mkdir -p "$out/docs" "$out/templates"

read -r -a binaries <<< "$(binary_names_for_platform "$platform")"
for binary in "${binaries[@]}"; do
  source_binary="$source/target/$target/release/$binary"
  if [[ ! -f "$source_binary" ]]; then
    echo "fetch_contextmink: built binary missing: $source_binary" >&2
    exit 68
  fi
  cp "$source_binary" "$out/"
  chmod +x "$out/$binary"
done

cp "$source/README.md" "$source/SETUP.md" \
  "$source/LICENSE" "$source/LICENSE-SSL" "$source/LICENSE-VPL" "$out/"
cp -R "$source/docs/." "$out/docs/"
cp -R "$source/templates/." "$out/templates/"

{
  printf '{\n'
  printf '  "name": "contextmink",\n'
  printf '  "version": "%s",\n' "$version"
  printf '  "target": "%s",\n' "$target"
  printf '  "platform": "%s",\n' "$platform"
  printf '  "binary": "%s",\n' "${binaries[0]}"
  if [[ "${#binaries[@]}" -gt 1 ]]; then
    printf '  "bridge_binary": "%s",\n' "${binaries[1]}"
  fi
  printf '  "archive": "vendored-source"\n'
  printf '}\n'
} > "$out/manifest.json"

echo "contextmink ${version} (${platform}, ${target}) -> $out"

if [[ "$install" -eq 1 ]]; then
  mkdir -p tools/contextmink/bin
  for binary in "${binaries[@]}"; do
    install_binary "$out/$binary" "tools/contextmink/bin/$binary"
  done
  cp "$out/templates/scripts/contextmink" scripts/contextmink
  chmod +x scripts/contextmink
  echo "installed: tools/contextmink/bin + scripts/contextmink"
fi
