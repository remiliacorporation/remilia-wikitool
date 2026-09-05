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

1. `wikitool.skills-manifest.vN` and `wikitool.skills-install.vN`
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

1. Stage the pinned upstream Contextmink and Papertiger packs with `bash scripts/fetch_contextmink.sh --all` and `bash scripts/fetch_papertiger.sh --all`, then use `cargo run --package wikitool --features maintainer -- release build-matrix --contextmink-dist dist/contextmink-dist --papertiger-dist dist/papertiger-dist` from a source checkout to emit per-target zip bundles.
2. Bundles use the embedded generic adapter by default, include the built-in generic and Remilia
   Wiki adapter catalog plus the deterministic `skills/` distribution, and compile the packaged binary
   without the maintainer surface. Catalog presence does not select an adapter.
3. A project adapter supplement is opt-in via `--host-project-root <PATH>`. It is packaged under
   `site_adapters/project/` and never replaces the public guidance, skills, or built-in catalog.

## Manual release checklist

1. Pick next version using rules above.
2. Update version in `Cargo.toml` workspace package.
3. Move the `[Unreleased]` notes in `CHANGELOG.md` under a dated `## [x.y.z] - date` heading.
4. Run:
   - `cargo build --workspace`
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo test --workspace --all-targets --all-features`
   - `cargo build -p wikitool -p wikitest` (default end-user feature set)
   - `target/debug/wikitest validate`
   - `target/debug/wikitest suite wikitool-regressions --require-all`

The broader capability and prose campaigns are not release gates. Run `wikitest suite wikitool-capabilities --require-all`
when a capability change warrants the full deterministic campaign, and schedule an external
`complex-prose-stress` campaign for substantive authoring/evaluator changes. Packet preparation by
itself is not a pass and must never be used to bless a release.

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
6. Build release bundles:
   - `bash scripts/fetch_contextmink.sh --platform <platform> --dest dist/contextmink-dist`
   - `bash scripts/fetch_papertiger.sh --platform <platform> --dest dist/papertiger-dist`
   - `cargo run --package wikitool --features maintainer -- release build-matrix --targets <triple> --contextmink-dist dist/contextmink-dist --papertiger-dist dist/papertiger-dist`
   - or run GitHub workflow `.github/workflows/release-artifacts.yml` with `artifact_version=X.Y.Z` for per-platform artifacts
   - every GitHub macOS artifact is explicitly marked unsigned and carries the bounded,
     checksum-first Gatekeeper procedure
7. Verify each zip contains:
   - `wikitool` or `wikitool.exe`
   - `README.md`
   - `skills/manifest.json`, whose complete file inventory validates with `wikitool skills inspect`
   - `skills/`, including `wiki-writing`, `prose-review`, `wiki-interview`, and `wikitool`
   - no source-checkout `AGENTS.md`/`CLAUDE.md`, generated `.claude/` wrappers, or legacy `codex_skills/`
   - `site_adapters/generic/site-adapter.toml`
   - `site_adapters/remilia-wiki/site-adapter.toml` and its declared guidance/template contracts
   - `site_adapters/project/site-adapter.toml` only when `--host-project-root` was supplied
   - `docs/wikitool/`
   - `contextmink/` with `contextmink` or `contextmink.exe`
   - `contextmink/contextmink-bridge.exe` in the Windows bundle only
   - `contextmink/archive.sha256`, matching the repository-pinned upstream archive receipt
   - `papertiger/` with both `papertiger` and `papertiger-mise` (`.exe` on Windows), its canonical
     agent contract, release manifest, licenses, and repository-pinned `archive.sha256` receipt
   - `release-companions.json`, identifying both external packs as optional and preserving their
     independent lifecycle ownership
   - no project install receipt; `.wikitool-skills/project-install.json` is created only by an
     explicit `wikitool skills setup-project`
8. Verify `SHA256SUMS.txt` matches the uploaded zip assets.
9. Create tag `X.Y.Z`.
