# Release Log

Chronological release notes for tagged wikitool versions.

## v0.3.0

Date: 2026-05-28

A structural release. No database reset required; v0.2.0 runtime state carries forward.

### What changed

v0.2.0 proved the knowledge layer; v0.3.0 reshapes the tool around it. The largest source files were split into focused modules (sync, retrieval, reference parsing, template catalog, wiki capabilities, HTML extraction, docs, and the CLI command families), command boundaries were tightened, and legacy public affordances were removed outright rather than aliased.

### New capabilities

- **First-run setup is one command.** `wikitool workflow session-refresh` initializes the runtime layout, pulls content, warms the knowledge index, and syncs the wiki profile in a single pass — and is now part of the end-user surface, not a hidden maintainer command. `workflow full-refresh` rebuilds local state from scratch.
- **Token-efficient brief outputs.** `--view brief` produces compact, interpreted reports for `knowledge article-start`, `knowledge inspect chunks`, `templates`, `wiki surface`, and `review`, so agents spend fewer tokens on retrieval substrate.
- **Research session handoff.** Import browser cookies (header, JSON bookmarklet, or Netscape file) so `research fetch` can reach session-gated sources.
- **Raw web archive crawler.** `research archive` captures a site's pages and requisites to disk, bounded by page count, per-response size, link depth, and an aggregate byte budget.
- **Maintainer audit lane.** `docs audit` (maintainer surface) verifies the generated CLI reference is current and that packaged guidance and skills stay aligned with the shipped command surface.

### Improvements

- Conflict hydration deduplicates titles before fetching remote timestamps, and `--force` skips the timestamp fetch it would ignore.
- The `maintainer-surface` build feature is now simply `maintainer`.
- Access-challenge detection adds Cloudflare/DataDome/Anubis fingerprints and treats generic markers (a lone "challenge-container", "captcha") as weak signals requiring corroboration, reducing false positives.

## v0.2.0

Date: 2026-03-18

Breaking release that replaces the retrieval layer with a purpose-built knowledge system for AI-assisted authoring.
Just delete your old wikitool installation ;d

### What changed

The core idea: wikitool's local index should give an AI agent everything it needs to write a good wiki article in one call. v0.1.0 had the raw materials — page chunks, template data, link graphs — but left the agent to assemble them. v0.2.0 introduces `knowledge article-start`, which interprets those materials into an opinionated authoring brief: which sections comparable articles use, which templates and categories apply, what type of subject this is, and where the evidence gaps are.

### Breaking changes

- **Database reset required.** The knowledge index schema is incompatible with v0.1.0. Delete `.wikitool/data/wikitool.db` and rebuild with `wikitool pull --full --all && wikitool knowledge warm --docs-profile remilia-mw-1.44 --docs-mode missing`.
- **Removed commands:** `workflow ask`, `workflow authoring-pack`, `db sync`, `index rebuild`. Use `knowledge article-start`, `knowledge pack`, and `knowledge build` respectively.
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

**Search & context** — FTS5 full-text search. Cross-page chunk retrieval with token budgeting. Authoring knowledge packs.

**Validation** — Broken link scanning, Lua module linting via Selene, text and JSON report export.

**External wiki tools** — Fetch wikitext or rendered HTML from any MediaWiki site. Export page trees. Bulk import from CSV/JSON.

**Documentation** — Import and search MediaWiki extension docs offline. Offline docs bundle import/export.

**Link analysis** — Backlinks, orphan detection, empty category pruning.

**Inspection** — SEO metatag inspection, network resource analysis.

### Technical

- Rust 2024 edition, bundled SQLite with FTS5, rustls (no OpenSSL)
- 78 unit tests, 36 CLI regression tests
- AGPL-3.0-only with supplementary terms
