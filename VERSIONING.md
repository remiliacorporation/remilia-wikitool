# Versioning Policy

This project uses SemVer for human-facing releases and separate schema versions for internal data contracts.

## Canonical release version

Format:

1. `X.Y.Z` for git tags and release notes
2. `X.Y.Z` in Cargo manifests

Artifact naming:

1. `wikitool-X.Y.Z-<target>.zip`
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

Example: `0.7.0` is a minor bump that intentionally replaces the old `knowledge`, `research`, and
`workflow` command buckets with explicit `catalog`, `article scout`, `source`, and `interview`
surfaces, and changes remote writes to preview/plan/apply contracts.

When CLI and bundle contracts stabilize, cut `1.0.0` and enforce strict SemVer from that point onward.

## Internal schema versioning

Schema versions are independent from SemVer and must be bumped only when their specific contract changes. The versioned families include:

1. `manifest.schema_version`
2. `ai/docs-bundle-vN.json`
3. `site_adapter_vN`
4. catalog, template-catalog, capability, and authoring-surface artifacts
5. `article_scout_vN`, article lint/fix/promote reports, and review changesets
6. `article_acceptance_ledger_vN` and the transactional acceptance-store schema
7. `wiki_interview_vN` and its command/report envelopes
8. the durable sync-store `user_version` and mutation-intent schemas
9. Wikitest scenario, run-receipt, and prose-evidence schemas

Catalog, docs, and other derived retrieval state are intentionally disposable. Current releases surface readiness through manifest-backed `runtime_artifacts` rows and the operator-facing `catalog_generation` contract. Sync baselines, mutation intents and receipts, and article-acceptance decisions are durable authority state: they require explicit, tested migrations and must fail closed when a schema is missing, corrupt, or unsupported. Catalog reset/rebuild operations must preserve those durable stores.

Cutover rule:

1. Do not add compatibility migrations for pre-manifest catalog databases.
2. Never repair a durable authority-store mismatch by deleting or rebuilding it; use a schema-owned migration or stop with a typed diagnostic.
3. Reset and rebuild derived state with `wikitool db reset --yes`, then `wikitool catalog build` or `wikitool catalog warm --docs-profile <PROFILE> --docs-mode missing`.
4. Use `wikitool catalog status --docs-profile <PROFILE>` to verify readiness before relying on local authoring retrieval.

## Release channels

Experimental / top-level steered:

1. Top-level repo can build and run latest submodule state directly:
   `cargo run --manifest-path tools/wikitool/Cargo.toml --package wikitool -- <command>`
2. This channel may include unreleased changes.

Packaged / distributable:

1. Stage the pinned upstream Contextmink packs with `bash scripts/fetch_contextmink.sh --all`, then use `cargo run --package wikitool --features maintainer -- release build-matrix --contextmink-dist dist/contextmink-dist` from a source checkout to emit per-target zip bundles.
2. Bundles are generic by default, include ai-pack baseline `.claude` + instruction files, and compile the packaged binary without the maintainer surface.
3. A project adapter supplement is opt-in via `--host-project-root <PATH>`. It is packaged under
   `site_adapter/project/` and never replaces the target-neutral public guidance or skills.

## Manual release checklist

1. Pick next version using rules above.
2. Update version in `Cargo.toml` workspace package.
3. Move the `[Unreleased]` notes in `CHANGELOG.md` under a dated `## [x.y.z] - date` heading.
4. Run:
   - `cargo build --workspace`
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo test --workspace --all-targets`
   - `cargo run --quiet --package wikitest -- validate`
   - `cargo run --quiet --package wikitest -- suite core-dogfood --require-all`
   - `cargo run --quiet --package wikitest -- prose prepare-suite prose-dogfood`
   - `bash tests/cli_compat/cli_tests.sh`
5. Validate the catalog and authoring-support cutover from a fresh runtime:
   - `cargo run --package wikitool -- db reset --yes`
   - `cargo run --package wikitool -- catalog warm --docs-profile mw-1.44-authoring --docs-mode missing`
   - `cargo run --package wikitool -- wiki capabilities sync`
   - `cargo run --package wikitool -- templates catalog build`
   - `cargo run --package wikitool -- catalog status --docs-profile mw-1.44-authoring`
   - `cargo run --package wikitool -- article scout "Example Topic" --docs-profile mw-1.44-authoring --format json`
   - `cargo run --package wikitool -- source wiki-search "Example Topic" --format json`
   - `cargo run --package wikitool -- article lint wiki_content/Main/Example_Topic.wiki --format json`
   - `cargo run --package wikitool -- catalog inspect references duplicates --title "Example Topic" --format json`
   - `cargo run --package wikitool -- status --conflicts --title "Example Topic"`
   - `cargo run --package wikitool -- module lint --format text`
   - `cargo run --package wikitool --features maintainer -- docs generate-reference`
6. If the release changes an editorial skill, prose schema, or packet construction, complete one
   real authoring assignment and its blinded review:
   - `target/debug/wikitest prose prepare aster-index-authoring`
   - have one external author follow the generated request, then use `prose submit-author`
   - have a differently identified reviewer work only from `review/export/`, then use
     `prose submit-review`
   - re-run `wikitest inspect <run>/receipt.json` and retain the verified receipt
7. Build release bundles:
   - `bash scripts/fetch_contextmink.sh --platform <platform> --dest dist/contextmink-dist`
   - `cargo run --package wikitool --features maintainer -- release build-matrix --targets <triple> --contextmink-dist dist/contextmink-dist`
   - or run GitHub workflow `.github/workflows/release-artifacts.yml` with `artifact_version=X.Y.Z` for per-platform artifacts
8. Verify each zip contains:
   - `wikitool` or `wikitool.exe`
   - `AGENTS.md`, `CLAUDE.md`, `README.md`
   - `.claude/rules/`, `.claude/skills/`
   - `codex_skills/`, including `wiki-writing`, `prose-review`, `wiki-interview`, and `wikitool-operator`
   - `integration/`
   - `site_adapter/generic.toml`
   - `site_adapter/project/site-adapter.toml` only when `--host-project-root` was supplied
   - `docs/wikitool/`
   - `contextmink/` with `contextmink` or `contextmink.exe`
   - `contextmink/contextmink-bridge.exe` in the Windows bundle only
   - `contextmink/archive.sha256`, matching the repository-pinned upstream archive receipt
   - `manifest.json`
9. Verify `SHA256SUMS.txt` matches the uploaded zip assets.
10. Create tag `X.Y.Z`.
