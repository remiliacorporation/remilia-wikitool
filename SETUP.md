# Wikitool Setup Guide

This guide gets a fresh clone ready for `wikitool`.

Read-only workflows do not require credentials. Push/delete writes require bot credentials.

## 1) Get the binary

Option A: download a release artifact for your OS and extract it. Community release bundles place the `wikitool` binary in the top-level extracted folder.

Option B: build from source:

```bash
cargo build --package wikitool --release
```

That source build keeps maintainer commands enabled. For a release-equivalent end-user binary, use:

```bash
cargo build --package wikitool --release --no-default-features
```

## 2) Initialize runtime

From the project root (or pass `--project-root`):

Release package:

```bash
wikitool init --templates
```

Source build from the `tools/wikitool/` checkout:

```bash
./target/release/wikitool init --templates
```

This materializes `.wikitool/` runtime state.

## 3) Pull content

Release package:

```bash
wikitool pull --full --all
```

Source build:

```bash
./target/release/wikitool pull --full --all
```

Incremental pull examples:

```bash
wikitool pull
wikitool pull --templates
wikitool pull --categories
```

## 4) Verify install

```bash
wikitool status
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44
wikitool knowledge article-start "Remilia Corporation" --format json
```

Useful authoring retrieval checks:

```bash
wikitool knowledge article-start "Remilia Corporation" --format json
wikitool research search "Remilia Corporation" --format json
wikitool templates show "Template:Cite web"
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
wikitool knowledge inspect references duplicates --title "Remilia Corporation" --format json
wikitool knowledge pack "Remilia Corporation" --format json
```

Command chooser:

- `knowledge build` for content-only local indexing
- `knowledge warm` for content indexing plus pinned docs hydration
- `knowledge status` to confirm readiness before depending on local retrieval
- `knowledge article-start` for the interpreted authoring brief
- `research search` and `research fetch` for external source discovery and extraction
- `article lint` and `article fix` for draft remediation
- `module lint` for Lua/module quality checks
- `knowledge pack` for advanced/raw context assembly
- `wiki profile sync|show` and `templates ...` for wiki-aware authoring surfaces
- `knowledge inspect ...` for low-level retrieval, graph inspection, and indexed reference audits
- `context` and `search` for quick indexed lookups against local wiki content
- `docs context` and `docs search` for pinned MediaWiki docs retrieval
- `docs ...` for direct docs import/search/context workflows

If you used older prerelease builds, replace `wikitool index ...` with `wikitool knowledge inspect ...`.

## 5) Optional: configure credentials and API target

Create `.env` in project root (next to `wiki_content/`) and set:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
WIKI_URL=https://your-wiki.example.org/
WIKI_API_URL=https://your-wiki.example.org/api.php
```

Prefer `.env` for shared repo-local settings. Treat `.wikitool/config.toml` as local materialized runtime state; it is often gitignored because it also records absolute local paths and accompanies the disposable DB.

Bot password setup:

1. Open `https://<your-wiki>/Special:BotPasswords`
2. Create a bot password with edit grants
3. Copy generated username/password into `.env`

## 6) Common workflow

```bash
wikitool diff
wikitool status --conflicts --title "Topic"
wikitool validate
wikitool push --dry-run --title "Topic" --summary "Summary"
wikitool push --title "Topic" --summary "Summary"
```

Default authoring loop:

```bash
wikitool knowledge article-start "Topic" --docs-profile remilia-mw-1.44 --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://example.org/source" --output json
wikitool article lint wiki_content/Main/Topic.wiki --format json
wikitool knowledge inspect references summary --title "Topic" --format json
wikitool status --modified --title "Topic" --format json
wikitool validate
```

## 7) Docs and AI pack

Canonical command docs:

- `docs/wikitool/reference.md`
- regenerate from a source checkout with the maintainer surface enabled:

```bash
cargo run --package wikitool -- docs generate-reference
```

AI companion source assets are maintained under `ai-pack/` in this repository.

Release AI pack includes setup/docs/instructions outside the binary. Bootstrap the pinned MediaWiki docs profile with:

```bash
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
wikitool knowledge article-start "Remilia Corporation" --docs-profile remilia-mw-1.44 --format json
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
wikitool knowledge inspect references duplicates --title "Remilia Corporation" --format json
```

If live installed-extension discovery is blocked or unconfigured, `remilia-mw-1.44` still imports the pinned core corpus and reports the discovery skip in the command output instead of aborting the whole import.

If `ai/docs-bundle-v1.json` is present, you can use it as an offline preload:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

Release assembly commands from a source checkout with the maintainer surface enabled:

```bash
cargo run --package wikitool -- release package
cargo run --package wikitool -- release build-matrix
```

`release build-matrix` uses versioned artifact names by default (`wikitool-vX.Y.Z-<target>.zip`).
For ephemeral CI-style output names, add `--unversioned-names`.

By default, release output is wikitool-generic and includes ai-pack `.claude/rules` and `.claude/skills`.
To layer host `.claude/rules`, host `.claude/skills`, and host `CLAUDE.md` on top, pass `--host-project-root <PATH>`.
Release bundles always ship the same guidance body as both `CLAUDE.md` and `AGENTS.md`.
When host overlay is used, wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.

Codex skill templates are also included under `codex_skills/` and can be copied into `$CODEX_HOME/skills`.
Focused acceptance checks also ship in `testbench/acceptance_workflows.sh`.

## 8) Troubleshooting

The local SQLite DB is disposable and recreated automatically on first use.

Authoring retrieval is DB-first and AI-oriented: semantic page profiles, normalized source authorities, identifier rows, template implementation bundles, module invocation patterns, pinned MediaWiki docs corpora, and explicit reference/template/link signals are indexed for retrieval, while local files remain the human editing surface.

If runtime/schema changes break local state:

1. Delete `.wikitool/data/wikitool.db`
2. Run `wikitool pull --full --all` if content is missing, then run `wikitool knowledge build` or `wikitool knowledge warm --docs-profile remilia-mw-1.44`

If push fails with auth errors, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
If installed-extension discovery or API-backed commands fail, verify `WIKI_URL` and `WIKI_API_URL` there first.
