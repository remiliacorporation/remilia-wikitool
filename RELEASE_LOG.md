# Release Log

Chronological release notes for tagged wikitool versions.

## v0.3.1

Date: 2026-05-28

Surface hardening for the Remilia-first default and agent-facing command contract.

### Breaking changes

- Removed bare `WIKI_*` environment variables. Use `WIKITOOL_WIKI_URL`, `WIKITOOL_WIKI_API_URL`, `WIKITOOL_USER_AGENT`, `WIKITOOL_ARTICLE_PATH`, `WIKITOOL_BOT_USER`, and `WIKITOOL_BOT_PASS`.
- Renamed the default docs profile from `remilia-mw-1.44` to `remilia-wiki`.
- Removed the public `knowledge pack` command and the raw-pack flags from `knowledge article-start`; use `knowledge article-start`, `knowledge contracts`, and `knowledge inspect` directly.

### Fixes

- `wikitool init` materializes Remilia Wiki by default while runtime resolution remains env > config > none.
- `wikitool config show` reports resolved wiki target values with their sources.
- `init --no-network` skips namespace discovery for offline/bootstrap runs.
- `validate --format json` keeps findings in the JSON status instead of using the process exit code for expected validation failures.
- MediaWiki search snippets decode HTML entities before emitting JSON.

## v0.3.0

Date: 2026-05-28

A consolidation release. No database reset required — v0.2.0 runtime state carries forward.

v0.2.0 introduced the knowledge layer; v0.3.0 builds the workflow around it and sharpens the agent-facing surface.

### New capabilities

- **One-command first-run setup.** `wikitool workflow session-refresh` initializes the local layout, pulls content, warms the knowledge index, and syncs the wiki's capability profile in a single pass. Run it again to refresh a session; use `workflow full-refresh` to rebuild from scratch.
- **Token-efficient `--view brief` outputs.** Compact, interpreted reports for `knowledge article-start`, `knowledge inspect chunks`, `templates`, `wiki surface`, and `review` — less raw retrieval substrate for an agent to wade through.
- **Research session handoff.** `research session` imports browser cookies (raw header, JSON bookmarklet, or Netscape file) so `research fetch` can read session-gated sources.
- **Web archive capture.** `research archive` saves a site's pages and assets to disk, bounded by page count, per-response size, link depth, and a total-bytes budget.

### Fixes

- Fetched content containing en/em dashes (`&ndash;` / `&mdash;`) now decodes correctly instead of emitting garbled characters.
- Access-challenge detection recognizes more anti-bot vendors (Cloudflare, DataDome, Anubis) and no longer flags ordinary pages that merely contain a generic marker such as "captcha".
- Push conflict checking is faster and steadier on large change sets.

### Other

- The source-build feature flag `maintainer-surface` is now simply `maintainer`.
- Substantial internal restructuring for maintainability, with no change to command behavior or output contracts.

## v0.2.0

Date: 2026-03-18

Breaking release that replaces the retrieval layer with a purpose-built knowledge system for AI-assisted authoring.
Just delete your old wikitool installation ;d

### What changed

The core idea: wikitool's local index should give an AI agent everything it needs to write a good wiki article in one call. v0.1.0 had the raw materials — page chunks, template data, link graphs — but left the agent to assemble them. v0.2.0 introduces `knowledge article-start`, which interprets those materials into an opinionated authoring brief: which sections comparable articles use, which templates and categories apply, what type of subject this is, and where the evidence gaps are.

### Breaking changes

- **Database reset required.** The knowledge index schema is incompatible with v0.1.0. Delete `.wikitool/data/wikitool.db` and rebuild with `wikitool pull --full --all && wikitool knowledge warm --docs-profile remilia-wiki --docs-mode missing`.
- **Removed commands:** `workflow ask`, `workflow authoring-pack`, `db sync`, `index rebuild`. Use `knowledge article-start` and `knowledge build` respectively.
- **Skill surface collapsed.** Nine Claude skills reduced to two: `/wikitool` (operator) and `/review` (content gate). The old skill names no longer resolve.

### New capabilities

**`knowledge article-start`** — The authoring front door. Returns an interpreted brief with:
- Section skeleton derived from comparable page structures, with `content_backed` flags indicating which sections have evidence in the retrieval pack and which need further research
- Subject type hints inferred from infobox usage across comparables
- Template, category, and link surfaces scoped to what similar articles actually use
- Hard constraints from the wiki's profile overlay
- Suggested next actions

**`knowledge build` / `warm` / `status` / `pack`** — Explicit knowledge lifecycle. `warm` builds the content index and hydrates a docs profile in one pass. `status` reports readiness and degradations so agents can tell the difference between missing content, missing docs, and a fully warmed corpus.

**`research wiki-search` / `research fetch`** — External evidence layer. Search the live wiki API and fetch URLs with structured metadata extraction.

**`article lint` / `article fix`** — Wiki-aware draft quality loop. Catches missing short descriptions, citation placement issues, heading case, and structural problems. `fix --apply safe` auto-corrects mechanical issues.

**`wiki profile sync` / `show`** — Live capability inspection. Exposes installed extensions, configured namespaces, and the active Remilia overlay.

**`templates show` / `examples`** — Template catalog. Shows parameters, usage stats, documentation, implementation pages, and real usage examples from the indexed content.

### Improvements

- Section skeleton now extracts headings directly from comparable pages via the content index, rather than relying on whatever headings happened to appear in retrieved chunks. Skeletons for well-covered topics went from 2 sections (Overview + References) to 5-8 meaningful sections.
- Every top-level CLI command now has a help description.
- Agent guidance (`article_structure.md`, `writing_guide.md`) updated to document skeleton interpretation: use as starting point, drop inapplicable sections, investigate `content_backed: false` gaps with `inspect chunks`.
- Retrieval internals split from a monolithic module into focused subsystems: content indexing, references, templates, retrieval, authoring, docs bridge, status.
- Knowledge artifacts are manifest-backed with schema generation tracking, so stale indexes are detected rather than silently degraded.
- Docs bridge enriches authoring retrieval with pinned MediaWiki 1.44 documentation, blending "how MediaWiki says it works" with "how this wiki uses it."
- 146 unit tests (up from 78). CLI regression testbench and acceptance workflow harness expanded.

## v0.1.0

Date: 2026-02-21

First public release. Single self-contained binary per platform with bundled AI companion pack.

### Features

**Sync & editing** — Pull articles, templates, and categories from any MediaWiki wiki. Push changes with conflict detection, dry-run preview, and edit summaries. Diff, status, and delete with backup support.

**Search & context** — FTS5 full-text search. Cross-page chunk retrieval with token budgeting. Authoring briefs.

**Validation** — Broken link scanning, Lua module linting via Selene, text and JSON report export.

**External wiki tools** — Fetch wikitext or rendered HTML from any MediaWiki site. Export page trees. Bulk import from CSV/JSON.

**Documentation** — Import and search MediaWiki extension docs offline. Offline docs bundle import/export.

**Link analysis** — Backlinks, orphan detection, empty category pruning.

**Inspection** — SEO metatag inspection, network resource analysis.

### Technical

- Rust 2024 edition, bundled SQLite with FTS5, rustls (no OpenSSL)
- 78 unit tests, 36 CLI regression tests
- AGPL-3.0-only with supplementary terms
