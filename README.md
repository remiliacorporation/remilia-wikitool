# Wikitool

Unified MediaWiki tooling for file-backed wiki workflows.

Primary runtime is a Rust CLI binary (`wikitool`) on stable Rust (edition 2024).

## Quick Start

From this repository:

```bash
cargo build --package wikitool --release
./target/release/wikitool init --templates
./target/release/wikitool pull --full --all
```

From a release package:

```bash
wikitool init --templates
wikitool pull --full --all
```

## Default Authoring Workflow

For real authoring work, the recommended front door is now:

```bash
wikitool init --templates
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge article-start "Remilia Corporation" --format json
wikitool research search "Remilia Corporation" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
```

Use `wikitool knowledge pack ...` when you need the deeper raw retrieval substrate behind `article-start`, not as the default first step.
`wikitool fetch` and `wikitool export` accept `/wiki/...`, `index.php?title=...`, `/w/index.php?title=...`, and base-path MediaWiki URLs.

## Runtime Layout

Wikitool resolves paths from project root and uses `.wikitool/` for local runtime state.

- `wiki_content/` local page files
- `templates/` local template/module files
- `.wikitool/config.toml` runtime config
- `.wikitool/data/wikitool.db` local index/sync database
- project root `.env` shared runtime overrides loaded automatically when present

The local SQLite DB is disposable and is created automatically on first use. If binary/schema changes make it stale, run `wikitool db reset --yes` or delete `.wikitool/data/wikitool.db`, then rerun `wikitool pull --full --all` if needed and rebuild retrieval state with `wikitool knowledge build` or `wikitool knowledge warm`.

For authoring retrieval, the DB is optimized as an AI-facing index rather than a human-facing store:

- local files remain the source of truth for editors
- SQLite stores semantic page profiles, links, sections, templates, module invocation patterns, references, source authorities, identifiers, media, template implementation relationships, and pinned docs corpora for fast retrieval
- reference rows expose explicit retrieval signals, normalized authority/identifier data, and source metadata instead of opaque quality scores
- active template lookup includes implementation bundles across template pages, `/doc` pages, helper templates, and `Module:` pages when present
- authoring retrieval can bridge pinned MediaWiki 1.44 docs with local template/module usage so agents get both “how MediaWiki says it works” and “how this wiki uses it”

## Core Sync Workflow

```bash
wikitool init --templates
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Edit summary"
wikitool push --summary "Edit summary"
```

## Namespaces

By default pull/push operate on Main namespace. Use flags for others:

- `--categories` for `Category:`
- `--templates` for Template/Module/MediaWiki namespaces
- `--all` (pull) for all supported namespaces

## AI Companion Pack

Source files for release AI companion packaging live under `ai-pack/`.

CI publishes zipped release artifacts (`wikitool-release-<target>`) where each zip unpacks into `wikitool-<target>/` with the binary and AI companion files in one folder.

Maintainer command for multi-target bundles:

```bash
wikitool release build-matrix
```

By default this emits versioned bundle names, for example:

1. `wikitool-v0.2.0-x86_64-unknown-linux-gnu.zip`
2. `wikitool-v0.2.0-x86_64-pc-windows-msvc.zip`

For CI matrix jobs, package one target explicitly:

```bash
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --unversioned-names
```

Manual multi-OS artifact builds are also available via GitHub Actions:

1. Run workflow: `.github/workflows/release-artifacts.yml`
2. Provide `artifact_version` (for example `v0.2.0`)
3. Download separate artifacts for:
   - `x86_64-pc-windows-msvc`
   - `x86_64-unknown-linux-gnu`
   - `x86_64-apple-darwin`

Release folder contents:

1. `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
2. `LICENSE`, `LICENSE-SSL`, `LICENSE-VPL`
3. `.claude/rules/*`, `.claude/skills/*` (baseline ai-pack guidance)
4. `llm_instructions/*.md`
5. `docs/wikitool/*.md`
6. `codex_skills/*` installable Codex skill bundle
7. optional `ai/docs-bundle-v1.json` for offline docs preload or fixtures (generated at build time, not committed)
8. optional host overlay extras when `--host-project-root` is provided:
   - host `CLAUDE.md` (mirrored to `AGENTS.md`)
   - `WIKITOOL_CLAUDE.md` preserving wikitool-local guidance
   - host `.claude/{rules,skills}` merged over baseline

This content is intentionally shipped outside the binary.

Bootstrap the local knowledge index and pinned MediaWiki authoring corpus:

```bash
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
wikitool knowledge article-start "Remilia Corporation" --docs-profile remilia-mw-1.44 --format json
wikitool research search "Remilia Corporation" --format json
wikitool templates show "Template:Cite web"
wikitool wiki profile show --format json
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
wikitool knowledge pack "Remilia Corporation" --docs-profile remilia-mw-1.44 --format json
wikitool docs context "parser function" --profile remilia-mw-1.44 --format json
wikitool templates examples "Template:Cite web" --limit 2
```

`remilia-mw-1.44` will try to enrich the corpus with installed extensions from the configured wiki when that API is available. If extension discovery is unavailable, the core pinned corpus still imports and remains usable for authoring retrieval.

Command chooser:

- `wikitool knowledge build` rebuilds only the local content index when docs hydration is unnecessary
- `wikitool knowledge warm` builds the local knowledge index and hydrates a docs profile in one pass
- `wikitool knowledge status` reports readiness, degradations, requested docs profile, and generation
- `wikitool knowledge article-start` is the primary AI/operator authoring front door
- `wikitool research search` and `wikitool research fetch` are the supported external evidence layer
- `wikitool article lint` and `wikitool article fix` are the draft quality loop
- `wikitool knowledge pack` is the advanced/raw substrate behind `article-start`
- `wikitool wiki profile sync|show` exposes the live capability plus Remilia overlay snapshot
- `wikitool templates show|examples` exposes the local template catalog and examples
- `wikitool knowledge inspect ...` is the low-level inspection lane for chunks, backlinks, templates, orphan pages, or empty categories
- `wikitool context` and `wikitool search` are quick indexed lookups against the local wiki knowledge index
- `wikitool docs context` and `wikitool docs search` query pinned MediaWiki docs corpora rather than local wiki pages
- `wikitool docs ...` remains the expert/admin surface for importing and managing pinned MediaWiki docs corpora directly
- `wikitool validate`, `wikitool diff`, and `wikitool push --dry-run` remain the low-level safety gates before writes

Legacy command mapping:

- `wikitool index stats` -> `wikitool knowledge inspect stats`
- `wikitool index chunks ...` -> `wikitool knowledge inspect chunks ...`
- `wikitool index backlinks ...` -> `wikitool knowledge inspect backlinks ...`
- `wikitool index templates ...` -> `wikitool knowledge inspect templates ...`
- `wikitool index orphans` -> `wikitool knowledge inspect orphans`
- `wikitool index prune-categories` -> `wikitool knowledge inspect empty-categories`

Use bundle import only when you need an offline preload:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

By default, release bundles stay wikitool-generic while still including the ai-pack `.claude` baseline.
If `--host-project-root` is provided, host context is layered on top and wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.

## Documentation

- `SETUP.md` setup guide
- `docs/wikitool/guide.md` workflows and troubleshooting
- `docs/wikitool/reference.md` command reference (auto-generated from CLI help)
- `VERSIONING.md` version bump policy and release checklist
- `RELEASE_LOG.md` release history
- `testbench/cli_tests.sh` broad CLI regression harness
- `testbench/acceptance_workflows.sh` focused authoring/workflow acceptance harness

Regenerate reference docs:

```bash
wikitool docs generate-reference
```

## Environment

Set these in `.env` at your project root (next to `wiki_content/`).

Prefer keeping shared project target settings in `.env` and treat `.wikitool/config.toml` as local materialized runtime state. `.wikitool/` is commonly gitignored because it also contains absolute local paths and the disposable SQLite DB.

Required for push/delete:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

Recommended for portable read/write API access:

```bash
WIKI_URL=https://your-wiki.example.org/
WIKI_API_URL=https://your-wiki.example.org/api.php
```

Environment variables override `.wikitool/config.toml`, so the same `wikitool` build can be reused across different MediaWiki projects without baking a specific domain into the repo or binary.

Optional tuning:

```bash
WIKI_HTTP_TIMEOUT_MS=30000
WIKI_HTTP_RETRIES=2
WIKI_HTTP_WRITE_RETRIES=1
WIKI_HTTP_RETRY_DELAY_MS=500
```

## License

AGPL-3.0-only. See `LICENSE`. Supplementary terms in `LICENSE-SSL` and `LICENSE-VPL`.
