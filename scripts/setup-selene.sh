#!/usr/bin/env bash
set -euo pipefail

VERSION="${SELENE_VERSION:-}"
URL="${SELENE_URL:-}"
SHA256="${SELENE_SHA256:-}"
FORCE=0

for arg in "$@"; do
  case "$arg" in
    --force|--rebuild|--fix)
      FORCE=1
      ;;
    *)
      echo "Unknown option: $arg"
      exit 1
      ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -d "$root/custom/wikitool" ]]; then
  wikitool="$root/custom/wikitool"
else
  wikitool="$root"
fi
tools_dir="$wikitool/tools"
mkdir -p "$tools_dir"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin)
    if [[ "$arch" == "arm64" ]]; then
      target="aarch64-apple-darwin"
    else
      target="x86_64-apple-darwin"
    fi
    binary_name="selene"
    ;;
  Linux)
    case "$arch" in
      x86_64|amd64)
        target="x86_64-unknown-linux-gnu"
        ;;
      aarch64|arm64)
        target="aarch64-unknown-linux-gnu"
        ;;
      *)
        echo "Unsupported Linux architecture: $arch"
        exit 1
        ;;
    esac
    binary_name="selene"
    ;;
  *)
    echo "Unsupported OS: $os"
    exit 1
    ;;
esac

dest="$tools_dir/$binary_name"
if [[ -f "$dest" && "$FORCE" -eq 0 ]]; then
  echo "Selene already installed at $dest"
  exit 0
fi
if [[ -f "$dest" ]]; then
  rm -f "$dest"
fi

if [[ -z "$URL" ]]; then
  if [[ -z "$VERSION" ]]; then
    asset="selene-${target}.zip"
    URL="https://github.com/Kampfkarren/selene/releases/latest/download/${asset}"
  else
    asset="selene-${VERSION}-${target}.zip"
    URL="https://github.com/Kampfkarren/selene/releases/download/${VERSION}/${asset}"
  fi
fi

tmp_zip="$tools_dir/selene.zip"
echo "Downloading Selene from $URL..."

if command -v curl >/dev/null 2>&1; then
  curl -L "$URL" -o "$tmp_zip"
elif command -v wget >/dev/null 2>&1; then
  wget -O "$tmp_zip" "$URL"
else
  echo "curl or wget required to download Selene."
  exit 1
fi

if [[ -n "$SHA256" ]]; then
  if command -v sha256sum >/dev/null 2>&1; then
    checksum="$(sha256sum "$tmp_zip" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    checksum="$(shasum -a 256 "$tmp_zip" | awk '{print $1}')"
  else
    echo "sha256sum or shasum required to verify checksum."
    exit 1
  fi
  checksum="$(printf '%s' "$checksum" | tr '[:upper:]' '[:lower:]')"
  expected="$(printf '%s' "$SHA256" | tr '[:upper:]' '[:lower:]')"
  if [[ "$checksum" != "$expected" ]]; then
    echo "Checksum mismatch. Expected $SHA256, got $checksum."
    exit 1
  fi
fi

if command -v unzip >/dev/null 2>&1; then
  unzip -o "$tmp_zip" -d "$tools_dir" >/dev/null
else
  echo "unzip is required to extract Selene."
  exit 1
fi

rm -f "$tmp_zip"

found="$(find "$tools_dir" -type f -name "selene" -o -name "selene.exe" | head -n 1)"
if [[ -z "$found" ]]; then
  echo "Selene binary not found after extraction."
  exit 1
fi

if [[ "$found" != "$dest" ]]; then
  mv "$found" "$dest"
fi
chmod +x "$dest"
echo "Selene installed to $dest"
