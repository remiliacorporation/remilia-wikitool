# AI Pack Source

This directory is the canonical source for AI companion content shipped with release artifacts.

Contents:

1. `AGENTS.md`
2. `CLAUDE.md`
3. `llm_instructions/*.md`
4. `codex_skills/*`
5. optional `docs-bundle-v1.json` (schema version `1` for `wikitool docs import --bundle`)

Packaging contract:

1. `scripts/build-ai-pack.sh` and `scripts/build-ai-pack.ps1` copy AI content from `ai-pack/`.
2. `scripts/package-release.sh` and `scripts/package-release.ps1` produce a single unzip-ready release folder where `wikitool` and AI companion files sit side by side.
3. Host project context can be overlaid when detected: host `CLAUDE.md` + host `.claude/{rules,skills}`.
4. Wikitool-local AI guidance is preserved as `WIKITOOL_CLAUDE.md` when host context is injected.
