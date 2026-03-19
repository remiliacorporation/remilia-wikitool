# Wikitool

CLI for MediaWiki editing, retrieval, and content management. Built to be wielded by AI agents — or just by you.

Single binary, bundled SQLite, no system dependencies. Ships with an AI companion pack so Claude Code and Codex agents can use it out of the box.

## Quick start

Download a release zip for your platform and extract it, or build from source:

```bash
cargo build --package wikitool --release
```

Then:

```bash
wikitool init --templates
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44
```

You're ready to write. See `SETUP.md` for credentials and configuration.

## What it does

**Write articles** — `knowledge article-start` assembles an interpreted authoring brief: comparable pages, section skeleton, template/category/link surfaces, type hints, and constraints. One command gives an agent (or you) everything needed to draft or improve an article.

**Sync content** — Pull articles, templates, and categories to local files. Edit locally, inspect scoped status/diff, then push back with remote-aware dry-run preflight and conflict detection.

**Research** — Full-text search across local content. Cross-page chunk retrieval with token budgeting. External web search and fetch with structured extraction.

**Validate** — Article-aware lint and mechanical fix. Structural integrity checks. Broken link and orphan detection. Lua module linting via `module lint`.

**Template and profile lookup** — Inspect any template's parameters, usage stats, and live examples. Query the wiki's capability profile and active extensions.

**Docs bridge** — Import MediaWiki extension documentation for offline reference. Authoring retrieval can blend "how MediaWiki says it works" with "how this wiki uses it."

## Authoring workflow

```bash
wikitool knowledge article-start "Topic" --format json   # interpreted brief
wikitool research search "Topic" --format json            # external evidence
wikitool templates show "Template:Infobox person"         # template params
# write the article
wikitool article lint wiki_content/Main/Topic.wiki --format json
wikitool knowledge inspect references duplicates --title "Topic" --format json
wikitool status --modified --title "Topic" --format json
wikitool validate
wikitool push --dry-run --title "Topic" --summary "Add article on Topic"
```

## AI companion pack

Every release includes agent guidance outside the binary:

- `CLAUDE.md` / `AGENTS.md` — canonical guidance for Claude Code and agent frameworks
- `.claude/skills/` — `/wikitool` operator and `/review` content gate
- `llm_instructions/` — writing guide, style rules, article structure, extensions reference
- `codex_skills/` — Codex-compatible skill definitions
- `docs/wikitool/` — operator guide and auto-generated command reference

## Runtime layout

```
project-root/
  .env                          # wiki credentials (WIKI_BOT_USER, WIKI_BOT_PASS, WIKI_URL)
  .wikitool/config.toml         # materialized runtime config
  .wikitool/data/wikitool.db    # local index (disposable — delete and rebuild any time)
  wiki_content/                 # pulled articles
  templates/                    # pulled templates and modules
```

## Environment

Set in `.env` at project root. Required for push/delete:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

Optional — override wiki target without editing config:

```bash
WIKI_URL=https://your-wiki.example.org/
WIKI_API_URL=https://your-wiki.example.org/api.php
```

## Documentation

| File | Purpose |
|------|---------|
| `SETUP.md` | Installation and first-run guide |
| `docs/wikitool/guide.md` | Workflows and troubleshooting |
| `docs/wikitool/reference.md` | Command reference (auto-generated) |
| `VERSIONING.md` | Version policy and release checklist |
| `RELEASE_LOG.md` | Release history |

Every command has `--help`. Regenerate the reference with `wikitool docs generate-reference`.

## Platforms

| Target | Archive |
|--------|---------|
| Windows x86_64 | `wikitool-v0.2.0-x86_64-pc-windows-msvc.zip` |
| Linux x86_64 | `wikitool-v0.2.0-x86_64-unknown-linux-gnu.zip` |
| macOS Intel | `wikitool-v0.2.0-x86_64-apple-darwin.zip` |
| macOS ARM | `wikitool-v0.2.0-aarch64-apple-darwin.zip` |

## Technical

- Rust 2024 edition, stable toolchain
- SQLite with FTS5, bundled (no system SQLite)
- HTTPS via rustls (no OpenSSL)
- 168 unit tests + CLI regression testbench
- AGPL-3.0-only (supplementary terms in `LICENSE-SSL`, `LICENSE-VPL`)
