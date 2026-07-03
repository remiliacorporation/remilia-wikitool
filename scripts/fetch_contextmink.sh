#!/usr/bin/env bash
set -euo pipefail

# Fetch the pinned contextmink release bundle for one platform and unpack it
# into <dest>/<platform>/ for `wikitool release ... --contextmink-dist <dest>`.
# The pin lives in config/contextmink.version. Requires the GitHub CLI.
#
# --local <checkout> builds the same bundle layout from a local contextmink
# source checkout instead of downloading a release, for iterating on both
# projects at once. The staged manifest carries the checkout's actual crate
# version: `--install` never consults the pin, while `wikitool release`
# still enforces manifest-version == pin, so a local release build fails
# loudly until the pin and the local version agree. CI stays on the pinned
# download path.

repo="remiliacorporation/contextmink"
dest="dist/contextmink-dist"
platform=""
install=0
local_checkout=""

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
    --local)
      local_checkout="${2:?--local requires a contextmink checkout path}"
      shift 2
      ;;
    --help | -h)
      echo "usage: fetch_contextmink.sh [--platform <windows-x86_64|linux-x86_64|macos-x86_64|macos-arm64>] [--dest <dir>] [--repo <owner/name>] [--local <checkout>] [--install]"
      echo "  --platform defaults to the host platform."
      echo "  --local builds the bundle from a local contextmink source checkout"
      echo "  (cargo build --release) instead of downloading the pinned release;"
      echo "  the staged manifest carries the checkout's actual version."
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

if [[ -n "$local_checkout" ]]; then
  if [[ ! -f "$local_checkout/Cargo.toml" ]] \
    || ! grep -q '^name = "contextmink"' "$local_checkout/Cargo.toml"; then
    echo "fetch_contextmink: $local_checkout is not a contextmink checkout (no Cargo.toml with name \"contextmink\")" >&2
    exit 65
  fi
  local_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$local_checkout/Cargo.toml" | head -1)"
  if [[ -z "$local_version" ]]; then
    echo "fetch_contextmink: could not read version from $local_checkout/Cargo.toml" >&2
    exit 65
  fi
  if [[ "$local_version" != "$version" ]]; then
    echo "fetch_contextmink: note: local checkout is ${local_version}, pin is ${version}; --install works, but 'wikitool release' will reject the bundle until they agree" >&2
  fi
  cargo build --release --manifest-path "$local_checkout/Cargo.toml"

  case "$platform" in
    windows-x86_64) binary_name="contextmink.exe" bridge_name="contextmink-bridge.exe" ;;
    *) binary_name="contextmink" bridge_name="" ;;
  esac
  target_triple="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"

  out="${dest}/${platform}"
  rm -rf "$out"
  mkdir -p "$out/docs" "$out/templates"
  cp "$local_checkout/target/release/$binary_name" "$out/"
  if [[ -n "$bridge_name" ]]; then
    cp "$local_checkout/target/release/$bridge_name" "$out/"
  fi
  cp "$local_checkout/README.md" "$local_checkout/SETUP.md" \
    "$local_checkout/LICENSE" "$local_checkout/LICENSE-SSL" "$local_checkout/LICENSE-VPL" "$out/"
  cp -R "$local_checkout/docs/." "$out/docs/"
  cp -R "$local_checkout/templates/." "$out/templates/"
  {
    printf '{\n'
    printf '  "name": "contextmink",\n'
    printf '  "version": "%s",\n' "$local_version"
    printf '  "target": "%s",\n' "${target_triple:-local}"
    printf '  "platform": "%s",\n' "$platform"
    printf '  "binary": "%s",\n' "$binary_name"
    if [[ -n "$bridge_name" ]]; then
      printf '  "bridge_binary": "%s",\n' "$bridge_name"
    fi
    printf '  "archive": "local-build"\n'
    printf '}\n'
  } > "$out/manifest.json"
  chmod +x "$out/$binary_name"
  echo "contextmink ${local_version} (local build, ${platform}) -> $out"
else

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
fi

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
