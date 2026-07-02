#!/usr/bin/env bash
set -euo pipefail

# Fetch the pinned contextmink release bundle for one platform and unpack it
# into <dest>/<platform>/ for `wikitool release ... --contextmink-dist <dest>`.
# The pin lives in config/contextmink.version. Requires the GitHub CLI.

repo="remiliacorporation/contextmink"
dest="dist/contextmink-dist"
platform=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)
      platform="${2:?--platform requires a value}"
      shift 2
      ;;
    --dest)
      dest="${2:?--dest requires a value}"
      shift 2
      ;;
    --repo)
      repo="${2:?--repo requires a value}"
      shift 2
      ;;
    --help | -h)
      echo "usage: fetch_contextmink.sh --platform <windows-x86_64|linux-x86_64|macos-x86_64|macos-arm64> [--dest <dir>] [--repo <owner/name>]"
      exit 0
      ;;
    *)
      echo "fetch_contextmink: unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [[ -z "$platform" ]]; then
  echo "fetch_contextmink: --platform is required" >&2
  exit 64
fi
if [[ ! -f config/contextmink.version ]]; then
  echo "fetch_contextmink: run from the wikitool repository root (config/contextmink.version not found)" >&2
  exit 65
fi
version="$(tr -d ' \t\r\n' < config/contextmink.version)"
if [[ -z "$version" ]]; then
  echo "fetch_contextmink: config/contextmink.version is empty" >&2
  exit 65
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

gh release download "v${version}" -R "$repo" \
  --pattern "contextmink-${version}-${platform}.*" -D "$workdir"

archive="$(find "$workdir" -maxdepth 1 \( -name "contextmink-${version}-${platform}.zip" -o -name "contextmink-${version}-${platform}.tar.gz" \) | head -1)"
if [[ -z "$archive" ]]; then
  echo "fetch_contextmink: no archive downloaded for ${platform} (does contextmink v${version} exist?)" >&2
  exit 66
fi

(
  cd "$workdir"
  checksum="$(basename "$archive").sha256"
  if [[ ! -f "$checksum" ]]; then
    echo "fetch_contextmink: checksum file missing for $(basename "$archive")" >&2
    exit 67
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$checksum"
  else
    shasum -a 256 -c "$checksum"
  fi
)

out="${dest}/${platform}"
rm -rf "$out"
mkdir -p "$out"
case "$archive" in
  *.zip) unzip -q "$archive" -d "$out" ;;
  *.tar.gz) tar -xzf "$archive" -C "$out" ;;
esac

# tar archives contain the bundle directory itself; flatten one level.
entries=("$out"/*)
if [[ ${#entries[@]} -eq 1 && -d "${entries[0]}" ]]; then
  mv "${entries[0]}"/* "$out"/
  rmdir "${entries[0]}"
fi

if [[ ! -f "$out/manifest.json" ]]; then
  echo "fetch_contextmink: unpacked bundle missing manifest.json in $out" >&2
  exit 68
fi
echo "contextmink ${version} (${platform}) -> $out"
