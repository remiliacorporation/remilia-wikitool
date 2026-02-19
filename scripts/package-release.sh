#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
binary_path="${1:-${repo_root}/target/release/wikitool}"
output_dir="${2:-${repo_root}/dist/release}"
ai_pack_dir="${repo_root}/dist/ai-pack"

if [[ ! -f "${binary_path}" ]]; then
  echo "Missing release binary: ${binary_path}" >&2
  exit 1
fi

bash "${repo_root}/scripts/build-ai-pack.sh" "${ai_pack_dir}"

if [[ -d "${output_dir}" ]]; then
  chmod -R u+w "${output_dir}" 2>/dev/null || true
  rm -rf "${output_dir}" 2>/dev/null || true
fi
mkdir -p "${output_dir}"

cp "${binary_path}" "${output_dir}/wikitool"
cp -R "${ai_pack_dir}/." "${output_dir}/"

echo "Packaged release at ${output_dir}"
