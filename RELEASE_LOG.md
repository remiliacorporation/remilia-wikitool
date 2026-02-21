# Release Log

Chronological release notes for tagged wikitool versions.

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
