# AI Companion Pack

This directory holds optional precomposed AI context artifacts that ship alongside release binaries.

Contract:

1. Files in `ai/` are not embedded into the Rust binary.
2. `scripts/build-ai-pack.sh` and `scripts/build-ai-pack.ps1` copy these artifacts into the release AI pack.
3. `scripts/package-release.sh` and `scripts/package-release.ps1` produce a single unzip-ready release folder where `wikitool` and AI companion files live side by side.
4. When a host project context is detected, host `CLAUDE.md` and host `.claude/{rules,skills}` are included in the release folder.
5. `codex_skills/` is included when present so Codex users can install project-tuned skill packs.
6. `ai/docs-bundle-v1.json` is optional:
   - when present, it should match schema version `1` for `wikitool docs import --bundle`.
   - when absent, users can still import docs live via `docs import` and `docs import-technical`.

Current status:

1. `docs-bundle-v1.json` is committed and built from current `llm_instructions/*.md`.
2. Release bundles include `wikitool(.exe)`, `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`, `llm_instructions/*.md`, `docs/wikitool/*.md`, optional host `.claude` context, `codex_skills/*`, and `ai/docs-bundle-v1.json`.
