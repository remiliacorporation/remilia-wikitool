#!/usr/bin/env bash
set -euo pipefail

# Fetch the pinned contextmink release bundle for one platform and unpack it
# into <dest>/<platform>/ for `wikitool release ... --contextmink-dist <dest>`.
# The pin lives in config/contextmink.version. Requires the GitHub CLI.

repo="remiliacorporation/contextmink"
dest="dist/contextmink-dist"
platform=""
install=0

host_platform() {
  case "$(uname -s 2>/dev/null):$(uname -m 2>/dev/null)" in
    Darwin:arm64) echo "macos-arm64" ;;
    Darwin:*) echo "macos-x86_64" ;;
    Linux:*) echo "linux-x86_64" ;;
    *) echo "windows-x86_64" ;;
  esac
}

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
    --install)
      install=1
      shift
      ;;
    --help | -h)
      echo "usage: fetch_contextmink.sh [--platform <windows-x86_64|linux-x86_64|macos-x86_64|macos-arm64>] [--dest <dir>] [--repo <owner/name>] [--install]"
      echo "  --platform defaults to the host platform."
      echo "  --install additionally places the binaries in tools/contextmink/bin/ and the"
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
  # The checksum file may carry CRLF when produced on Windows; strip CRs
  # before verifying so the listed filename resolves.
  if command -v sha256sum >/dev/null 2>&1; then
    tr -d '\r' < "$checksum" | sha256sum -c -
  else
    tr -d '\r' < "$checksum" | shasum -a 256 -c -
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

if [[ "$install" -eq 1 ]]; then
  mkdir -p tools/contextmink/bin
  for binary in contextmink contextmink.exe contextmink-bridge.exe; do
    if [[ -f "$out/$binary" ]]; then
      cp "$out/$binary" "tools/contextmink/bin/$binary"
      chmod +x "tools/contextmink/bin/$binary"
    fi
  done
  cp "$out/templates/scripts/contextmink" scripts/contextmink
  chmod +x scripts/contextmink
  echo "installed: tools/contextmink/bin + scripts/contextmink"
fi
