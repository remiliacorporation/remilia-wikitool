# AI Companion Pack

This directory holds optional precomposed AI context artifacts that ship alongside release binaries.

Contract:

1. Files in `ai/` are not embedded into the Rust binary.
2. `scripts/build-ai-pack.sh` and `scripts/build-ai-pack.ps1` copy these artifacts into the release AI pack.
3. `ai/docs-bundle-v1.json` is optional:
   - when present, it should match schema version `1` for `wikitool docs import --bundle`.
   - when absent, users can still import docs live via `docs import` and `docs import-technical`.

Current status:

1. No default `docs-bundle-v1.json` is committed yet.
2. Release AI packs still include `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`, `llm_instructions/*.md`, and `docs/wikitool/*.md`.
