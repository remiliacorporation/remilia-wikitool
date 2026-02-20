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

When CLI and bundle contracts stabilize, cut `1.0.0` and enforce strict SemVer from that point onward.

## Internal schema versioning

Schema versions are independent from SemVer and must be bumped only when their specific contract changes:

1. `manifest.schema_version`
2. `ai/docs-bundle-vN.json`

## Release channels

Experimental / top-level steered:

1. Top-level repo can build and run latest submodule state directly:
   `cargo run --manifest-path custom/wikitool/Cargo.toml --package wikitool -- <command>`
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
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo test --workspace`
5. Build release bundles:
   - `cargo run --package wikitool -- release build-matrix --targets <triple>`
   - or run GitHub workflow `.github/workflows/release-artifacts.yml` with `artifact_version=vX.Y.Z` for per-platform artifacts
6. Verify each zip contains:
   - `wikitool` or `wikitool.exe`
   - `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
   - `.claude/rules/`, `.claude/skills/`
   - `llm_instructions/`
   - `codex_skills/`
   - `manifest.json`
7. Create tag `vX.Y.Z`.
