# Versioning Policy

This project uses SemVer for human-facing releases and separate schema versions for internal data contracts.

## Canonical release version

Format:

1. `vX.Y.Z` for git tags and release notes
2. `X.Y.Z` in Cargo manifests

Artifact naming:

1. `wikitool-vX.Y.Z-<target>.zip`
2. Use `--unversioned-names` only for ephemeral CI/non-release artifacts.

## SemVer bump rules

Major (`X`):

1. Breaking CLI contract changes (command removals/renames, incompatible flag behavior)
2. Breaking release artifact contract changes (folder layout, required packaged AI files)
3. Breaking machine-consumed output contract changes used by automation

Minor (`Y`):

1. Backward-compatible command additions
2. Backward-compatible flag additions
3. Backward-compatible release bundle additions

Patch (`Z`):

1. Bug fixes without contract breaks
2. Internal refactors
3. Docs/test/CI fixes

## Pre-1.0 guidance

Current series is `0.y.z`. Before `1.0.0`, breaking changes may happen in minor bumps.

Example: `v0.2.0` is a minor bump that intentionally removed legacy retrieval commands in favor of the `knowledge` command family.

When CLI and bundle contracts stabilize, cut `1.0.0` and enforce strict SemVer from that point onward.

## Internal schema versioning

Schema versions are independent from SemVer and must be bumped only when their specific contract changes:

1. `manifest.schema_version`
2. `ai/docs-bundle-vN.json`

Local retrieval state is intentionally disposable. Starting with `v0.2.0`, readiness is surfaced through manifest-backed `knowledge_artifacts` rows and the operator-facing `knowledge_generation` contract.

Cutover rule:

1. Do not add compatibility migrations for pre-manifest knowledge databases.
2. Reset and rebuild local state with `wikitool db reset --yes`, then `wikitool knowledge build` or `wikitool knowledge warm --docs-profile <PROFILE>`.
3. Use `wikitool knowledge status --docs-profile <PROFILE>` to verify readiness before relying on local authoring retrieval.

## Release channels

Experimental / top-level steered:

1. Top-level repo can build and run latest submodule state directly:
   `cargo run --manifest-path tools/wikitool/Cargo.toml --package wikitool -- <command>`
2. This channel may include unreleased changes.

Packaged / distributable:

1. Use `wikitool release build-matrix` to emit per-target zip bundles.
2. Bundles are generic by default and include ai-pack baseline `.claude` + instruction files.
3. Host context overlay is opt-in via `--host-project-root <PATH>`.

## Manual release checklist

1. Pick next version using rules above.
2. Update version in `Cargo.toml` workspace package.
3. Update `RELEASE_LOG.md` with dated entry and highlights.
4. Run:
   - `cargo build`
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test -p wikitool_core`
   - `cargo test -p wikitool`
   - `bash testbench/cli_tests.sh`
   - `TIER=live bash testbench/acceptance_workflows.sh`
5. Validate the knowledge cutover from a fresh runtime:
   - `cargo run --package wikitool -- db reset --yes`
   - `cargo run --package wikitool -- knowledge warm --docs-profile remilia-mw-1.44`
   - `cargo run --package wikitool -- wiki profile sync`
   - `cargo run --package wikitool -- knowledge status --docs-profile remilia-mw-1.44`
   - `cargo run --package wikitool -- knowledge article-start "Example Topic" --docs-profile remilia-mw-1.44 --format json`
   - `cargo run --package wikitool -- research search "Example Topic" --format json`
   - `cargo run --package wikitool -- article lint wiki_content/Main/Example_Topic.wiki --format json`
   - `cargo run --package wikitool -- docs generate-reference`
6. Build release bundles:
   - `cargo run --package wikitool -- release build-matrix --targets <triple>`
   - or run GitHub workflow `.github/workflows/release-artifacts.yml` with `artifact_version=vX.Y.Z` for per-platform artifacts
7. Verify each zip contains:
   - `wikitool` or `wikitool.exe`
   - `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
   - `.claude/rules/`, `.claude/skills/`
   - `llm_instructions/`
   - `codex_skills/`
   - `manifest.json`
8. Create tag `vX.Y.Z`.
