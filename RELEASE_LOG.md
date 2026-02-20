# Release Log

Chronological release notes for tagged wikitool versions.

## Unreleased

Date: 2026-02-20

Highlights:

1. Added binary-native workflow/release/dev helpers to replace shell script wrappers.
2. Added `release build-matrix` for per-target zip bundles (`windows/linux/macos` target triples).
3. CI now publishes per-target zip artifacts + SHA256 checksums.
4. Release packaging host-context overlay changed to opt-in only via `--host-project-root`.
5. Re-aligned setup/reference/skill docs to Rust-only and binary-native command flows.
6. Generic release bundles now include ai-pack baseline `.claude/{rules,skills}` and enforce `AGENTS.md == CLAUDE.md`.
