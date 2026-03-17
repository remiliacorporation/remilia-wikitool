# Release Log

Chronological release notes for tagged wikitool versions.

## Unreleased (target: v0.2.0)

Breaking release focused on the knowledge-index cutover for high-performance local authoring retrieval.

### What's new

The local retrieval path now centers on a dedicated `knowledge` command family. The SQLite database remains disposable, but knowledge readiness is now explicit and manifest-backed so agents can tell the difference between missing content, missing docs corpora, and fully warmed authoring context.

### Highlights

**Authoring workflow cutover**
- Added `knowledge article-start` as the documented front door for greenfield and refactor authoring work
- Added `research search` / `research fetch` as the supported external evidence layer
- Added `article lint` / `article fix` for wiki-aware draft remediation
- Added `wiki profile ...` and `templates ...` surfaces for live capability/profile/template introspection
- Cut README, setup docs, how-to docs, explanation docs, and shipped AI-pack guidance over to the new default workflow

**Knowledge cutover**
- Added `knowledge build`, `knowledge warm`, `knowledge status`, and `knowledge pack`
- Removed legacy retrieval entry points: `workflow ask`, `workflow authoring-pack`, `db sync`, and `index rebuild`
- `workflow bootstrap` and `workflow full-refresh` now hydrate knowledge via `knowledge warm`
- `context` and `search` are indexed-only and report readiness errors when the local knowledge index is missing
- Pre-manifest populated databases are treated as incompatible cutover state; reset and rebuild instead of attempting in-place migration

**Retrieval internals**
- Split monolithic retrieval code into `knowledge::{content_index,references,templates,retrieval,authoring,docs_bridge,status}`
- Added `knowledge_artifacts` manifest rows for `content_index` and per-profile docs hydration
- Replaced the hard-coded docs profile path with explicit `--docs-profile` plumbing through build and pack flows
- Removed the orphaned duplicate docs implementation and kept `wikitool_core::docs` focused on docs import/search/context

**Operator visibility**
- `knowledge pack`, `knowledge status`, and `db stats` now expose `docs_profile_requested`, `readiness`, `degradations`, and `knowledge_generation`
- Missing docs corpora now surface as `docs_profile_missing` instead of silently degrading behind `docs_context: null`
- Fresh-runtime cutover is validated from `db reset --yes` through `knowledge warm --docs-profile remilia-mw-1.44`

**Acceptance coverage**
- `testbench/cli_tests.sh` now covers the article lint/fix surface inside the broad regression harness
- `testbench/acceptance_workflows.sh` adds targeted acceptance checks for `knowledge article-start`, `research search`, `research fetch`, `article lint`, and `wiki capabilities sync`
- `testbench/eval_matrix.md` records the workflow, quality, and latency eval cases used for operator usefulness review

## v0.1.0

Date: 2026-02-21

First public release. Single self-contained binary per platform (Windows, Linux, macOS) with bundled AI companion pack.

### What's new

wikitool is a CLI toolkit for wiki editing, validation, and content management. It's built to be wielded by Claude Code and other AI agents — or just by you. The release zip ships with built-in Claude skills reflecting every capability, so you can use it through `/wikitool` or by asking for wikitool features directly.

wikitool reduces AI context rot by storing pulled articles locally in a SQLite database, chunked by semantic type, making retrieval extremely token-efficient. Combined with live wiki fetching, this greatly improves AI-assisted article editing accuracy and proper use of wiki-specific knowledge and templates.

### Features

**Sync & editing**
- Pull articles, templates, and categories from any MediaWiki wiki to edit locally
- Push local changes back with edit summaries, conflict detection, and dry-run preview
- Diff local changes against last-synced state before pushing
- Status view showing modified, new, and conflicting pages
- Delete pages with backup and dry-run support

**Search & context**
- FTS5 full-text search across all local wiki content with substring matching
- Search external wikis (Wikipedia, Miraheze, any MediaWiki site)
- AI-ready context bundles for any page (metadata, links, sections, template params)
- Cross-page semantic chunk retrieval with token budgeting and diversification
- Authoring knowledge packs: one command to get everything needed to write or improve an article

**Validation & linting**
- Scan for broken links, missing references, and style issues
- Lint Lua modules with Selene integration
- Export reports in text or JSON

**External wiki tools**
- Fetch raw wikitext from any MediaWiki site
- Export any MediaWiki page to clean markdown (great for AI research context)
- Pull entire subpage trees into organized folders
- Bulk import from CSV/JSON into wiki pages

**Documentation**
- Import MediaWiki extension docs for offline reference
- Import technical docs (hooks, config settings, API) from mediawiki.org
- Offline docs bundle import/export for air-gapped setups
- Search imported docs locally without hitting the web

**Link analysis**
- Backlinks: find all pages linking to a specific page
- Orphan detection: find pages with no incoming links
- Category pruning: find empty categories

**Inspection**
- SEO metatag inspection on any page (not just your wiki)
- Network resource and cache header analysis
- Lighthouse performance audits with HTML/JSON reports

**Editor setup**
- Generate VS Code configuration for the Wikitext extension (wikiparser by Bhsd)
- Parser config auto-detection from wiki API (installed extensions, namespaces)

**Wiki-agnostic configuration**
- Config-driven wiki identity via `config.toml` — no hardcoded wiki URLs
- Custom namespace support from config or auto-discovered from wiki API
- Configurable article path pattern (`/$1` for short URLs, `/wiki/$1` for standard)
- Environment variable overrides for all settings (credentials stay env-only)
- Precedence: CLI flag > env var > config.toml > compiled default

**Release & developer tooling**
- Built-in multi-target release bundling (`release build-matrix`)
- AI companion pack assembly with optional host project overlay
- Git hook installer for commit message hygiene
- Contract snapshot and command-surface verification harness
- Automatic database schema migrations

### AI companion pack

Every release zip includes a complete AI companion pack:

- `CLAUDE.md` / `AGENTS.md` — canonical AI guidance
- `.claude/rules/` — safety, conventions, wiki style rules
- `.claude/skills/` — 9 slash commands (`/article`, `/template`, `/sync`, `/research`, `/cleanup`, `/review`, `/seo`, `/mw-fetch`, `/wikitool`)
- `llm_instructions/` — writing guide, style rules, article structure, extensions reference
- `codex_skills/` — Codex-compatible operator and content gate skills
- `docs/wikitool/` — operator documentation (Diataxis format)
- `manifest.json` — bundle metadata and feature flags

### Platforms

| Target | Archive |
|--------|---------|
| Windows x86_64 | `wikitool-v0.1.0-x86_64-pc-windows-msvc.zip` |
| Linux x86_64 | `wikitool-v0.1.0-x86_64-unknown-linux-gnu.zip` |
| macOS x86_64 (Intel) | `wikitool-v0.1.0-x86_64-apple-darwin.zip` |
| macOS ARM (Apple Silicon) | `wikitool-v0.1.0-aarch64-apple-darwin.zip` |

### Technical

- Rust edition 2024, stable toolchain
- SQLite with FTS5, WAL mode, bundled (no system SQLite needed)
- HTTPS via rustls (no OpenSSL dependency)
- 78 unit tests, 36 CLI regression tests
- License: AGPL-3.0-only with supplementary terms (SSL, VPL)
