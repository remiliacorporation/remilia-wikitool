#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${1:-${repo_root}/dist/ai-pack}"

if [[ -d "${output_dir}" ]]; then
  chmod -R u+w "${output_dir}" 2>/dev/null || true
  rm -rf "${output_dir}" 2>/dev/null || true
  if [[ -d "${output_dir}" ]]; then
    find "${output_dir}" -mindepth 1 -depth -exec rm -rf {} + 2>/dev/null || true
    rmdir "${output_dir}" 2>/dev/null || true
  fi
fi
if [[ -d "${output_dir}" ]]; then
  echo "Failed to clear output directory: ${output_dir}" >&2
  exit 1
fi
mkdir -p "${output_dir}"

required_files=(
  "AGENTS.md"
  "SETUP.md"
  "README.md"
)

for file in "${required_files[@]}"; do
  src="${repo_root}/${file}"
  if [[ ! -f "${src}" ]]; then
    echo "Missing required AI pack file: ${file}" >&2
    exit 1
  fi
  cp "${src}" "${output_dir}/"
done

# Default CLAUDE context is the wikitool-local guidance.
cp "${repo_root}/CLAUDE.md" "${output_dir}/CLAUDE.md"

# Optionally include parent project Claude context (.claude/rules + .claude/skills).
host_context_included=false
host_root="${WIKITOOL_HOST_PROJECT_ROOT:-}"
if [[ -z "${host_root}" ]]; then
  for candidate in "${repo_root}/../.." "${repo_root}/.." "${repo_root}/../../.."; do
    if [[ -f "${candidate}/CLAUDE.md" && -d "${candidate}/.claude/rules" && -d "${candidate}/.claude/skills" ]]; then
      host_root="$(cd "${candidate}" && pwd)"
      break
    fi
  done
fi

repo_root_abs="$(cd "${repo_root}" && pwd)"
if [[ -n "${host_root}" ]]; then
  host_root_abs="$(cd "${host_root}" && pwd)"
  if [[ "${host_root_abs}" != "${repo_root_abs}" && -f "${host_root_abs}/CLAUDE.md" ]]; then
    cp "${repo_root}/CLAUDE.md" "${output_dir}/WIKITOOL_CLAUDE.md"
    cp "${host_root_abs}/CLAUDE.md" "${output_dir}/CLAUDE.md"
    mkdir -p "${output_dir}/.claude"
    cp -R "${host_root_abs}/.claude/rules" "${output_dir}/.claude/rules"
    cp -R "${host_root_abs}/.claude/skills" "${output_dir}/.claude/skills"
    host_context_included=true
  fi
fi

mkdir -p "${output_dir}/llm_instructions"
llm_files=("${repo_root}"/llm_instructions/*.md)
if [[ ${#llm_files[@]} -eq 0 || ! -f "${llm_files[0]}" ]]; then
  echo "No llm_instructions/*.md files found" >&2
  exit 1
fi
cp "${repo_root}"/llm_instructions/*.md "${output_dir}/llm_instructions/"

if [[ -d "${repo_root}/docs/wikitool" ]]; then
  mkdir -p "${output_dir}/docs/wikitool"
  find "${repo_root}/docs/wikitool" -maxdepth 1 -type f -name "*.md" -exec cp {} "${output_dir}/docs/wikitool/" \;
fi

codex_skills_included=false
if [[ -d "${repo_root}/codex_skills" ]]; then
  mkdir -p "${output_dir}/codex_skills"
  cp -R "${repo_root}/codex_skills/." "${output_dir}/codex_skills/"
  codex_skills_included=true
fi

docs_bundle_included=false
if [[ -f "${repo_root}/ai/docs-bundle-v1.json" ]]; then
  mkdir -p "${output_dir}/ai"
  cp "${repo_root}/ai/docs-bundle-v1.json" "${output_dir}/ai/docs-bundle-v1.json"
  docs_bundle_included=true
fi

generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
cat > "${output_dir}/manifest.json" <<EOF
{
  "schema_version": 1,
  "generated_at_utc": "${generated_at}",
  "host_context_included": ${host_context_included},
  "codex_skills_included": ${codex_skills_included},
  "docs_bundle_included": ${docs_bundle_included},
  "notes": "AI companion pack for wikitool; content is intentionally shipped outside the binary."
}
EOF

echo "Built AI pack at ${output_dir}"
