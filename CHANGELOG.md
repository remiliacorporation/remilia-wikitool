# Changelog

All notable changes to wikitool are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

The release workflow extracts the section for the requested version and fails if it is missing, so land notes here (staged under Unreleased, then retitled) before dispatching a release. Write one line per paragraph or bullet: GitHub release bodies render every newline as a line break, so hard-wrapped prose comes out ragged.

## [Unreleased]

## [0.6.1] - 2026-07-07

### Changed

- Release artifact builds now normalize artifact labels to bare semver, emit `SHA256SUMS.txt`, run the packaged `wikitool` binary during CI release verification, and publish checksums with GitHub release assets.
- Release artifact verification now smoke-tests `wikitool contextmink install` from the packaged sibling `contextmink/` release pack on Unix and Windows, including the installed Windows bridge path.
- Contextmink setup guidance now explains invocation by active shell and target: Bash-hosted sessions use `scripts/contextmink ...`, Windows PowerShell uses the native `contextmink.exe` for direct contextmink commands, and `contextmink-bridge.exe --script scripts/contextmink ...` is the PowerShell-to-Git-Bash path for the Bash launcher.

### Fixed

- `wikitool contextmink install` now treats the vendored Contextmink source checkout as a first-class install source when wikitool is source-built, and `--from` accepts either a release pack or a source checkout.
- Source-checkout Contextmink installs now build missing `contextmink` binaries during real installs while keeping dry-runs non-mutating and explicit about `source_kind`; dry-run build deferral is limited to missing build outputs, so missing templates or launcher files fail instead of being mislabeled as buildable.
- `docs audit` now treats the live `wikitool contextmink` command as distinct from the retired bare context surface and checks release-note interview framing in `CHANGELOG.md` after the `RELEASE_LOG.md` migration.
- The vendored Contextmink launcher is synced with the canonical 0.6.0 launcher so it can find Cargo in common Windows and WSL layouts instead of relying only on a non-login `PATH`.

## [0.6.0] - 2026-07-06

This release consolidates the latest authoring, review, documentation, and release-bundling work into a public-ready wikitool package. The release artifacts are now self-contained: wikitool builds the vendored Contextmink 0.6.0 source for each target, including the Windows bridge, instead of depending on a post-release fetch script.

### Added

- Extension-tag contracts are now one data model instead of three drifting copies: `writing_context/extensions.md` carries a machine-readable `Contract:` line per content mechanism (tag, parser function, template, module) that the profile overlay parses, the authoring surface consumes (tags gain `documented`, `body_required`, and `attributes`, plus a drift warning when the doc teaches a tag the live wiki does not expose), and lint consumes (every body-required tag contract gains an empty-body check with no tag-specific code; documenting a new tag now documents, surfaces, and lints it in one edit).
- The interview question agenda now flags intent mismatches: an existing page under `--intent new` asks whether the work should be an expansion or a genuinely separate article, instead of proceeding toward a duplicate.
- `knowledge interview init` now runs the local authoring scout itself: it writes a tool-authored `Scout Context` section into the brief (local state, comparable pages, the closest comparable's outline, infobox candidates, categories, citation patterns, missing query terms) and returns an evidence-grounded `question_agenda` - suggested question areas with the evidence that motivates each, for the interviewer to adapt conversationally rather than follow as a script. When the wiki knows nothing the agenda opens with the freeform monologue; when a page or close comparable exists it steers to the delta and the shape question. `--no-scout` starts blank.
- Interview briefs now hand off more than the Draft Plan: `Recommended angle`, `Tone risks`, `Likely misconceptions`, and `Terminology notes` (Editorial Framing), `Blocking evidence gaps` (Research Plan), and `Related wiki pages` (Entities) are parsed as structured signals on the validation summary, and blocking gaps surface as warnings in `article-start --brief-path`.
- `interview validate` warns advisorily when a core section (Article Object, User-Framed Summary, Interview Transcript and Context, Editorial Framing, Research Plan, Draft Plan) is still at its template state, and the summary reports `sections_unfilled`; a freshly-inited blank brief now reads as `warning`, not `valid`. Chronology, Entities, Scope, and Initial Materials never draw fill warnings, per the playbook's do-not-force rule.
- `docs symbols` now carries real signatures and summaries for parser functions, tags, and magic words, extracted from the documenting section (e.g. `#cargo_query` shows a full `{{#cargo_query:tables=...|where=...}}` call instead of the bare name).
- The article-start docs bridge fires on what the draft actually invokes: parser functions (`{{#name:`) and capability-known extension tags detected in the stub now produce docs queries, prioritized ahead of template-derived queries; the old template-only gate is gone.
- `wikitool contextmink install` performs a deterministic contextmink setup in a wikitool project: it finds the pack shipped next to the wikitool binary (or takes `--from`), places the platform binaries, launcher, and guidance templates, generates a wikitool-tailored `.contextmink.toml` (broad scans skip `.wikitool/**`; explicit paths still work), and verifies the installed binary against the pack manifest. Standalone contextmink stays doc-driven because arbitrary repository layouts cannot be assumed; wikitool projects have a known layout, so the install is exact every time. Packaged guidance now routes agents through the installer with `contextmink/SETUP.md` as the manual fallback.
- `wiki cargo tables`, `wiki cargo fields <table>`, and `wiki cargo rows <table>` open the live Cargo lane an authoring agent was blind to: table list, typed field schema (with list markers and delimiters), and filtered row samples (`--field`, `--where`, `--order-by`, `--limit`, `--offset`). Row keys are aliased to match schema field names exactly.
- The article-start brief now carries the closest comparable page's section sequence in document order (`closest_comparable_outline`), template parameter keys inlined on infobox/template/required-template cards, and a `text_preview` on top evidence cards, so drafting starts without extra drill-down calls.
- `wiki surface` now surfaces the live wiki's parser functions (`{{#cargo_query:...}}`-style call hints plus docs queries) — they were synced into the capability manifest but invisible in every curated view — and each local module's source-declared exported functions, which previously only lint could see.
- Template parameter cards now carry TemplateData `example`, `default_value`, `suggested_values`, and `auto_value` where declared; these were parsed and then dropped at the catalog boundary.
- Extension tags in the surface brief carry their wrap syntax and docs query instead of bare names; the 60-entry static HTML-tag allow-list no longer pads every brief payload.

### Fixed

- `wikitool contextmink install` now writes relative to the current project/agent working directory unless `--project-root` is explicit, so a first-run install inside an unmarked directory cannot be captured by an unrelated initialized ancestor.
- Local contextmink staging now skips identical binary installs before copying, so reinstalling through `contextmink-bridge.exe` on Windows no longer trips over its own running executable.
- Blocking evidence gaps recorded in an interview brief now surface as blocking open questions in `article-start --brief-path`, forcing readiness to `not_ready` until resolved or deferred; previously they were only warnings, which readiness ignored - "blocking" did not block.
- Ubiquitous tags (`ref`, `nowiki`, `noinclude`, ...) no longer spend the docs bridge's four capped query slots; slots go to the tags and parser functions an agent actually needs documentation for.
- Extension and technical docs subpage listing passed the namespaced title as `apprefix` while also setting `apnamespace`, which silently matched nothing: every extension corpus was main-page-only and the technical sweeps (`Manual:Hooks/*`, `Manual:$wg*`, `API:*`, `Help:*`) could never enumerate. With the fix the Cargo corpus alone goes from 1 page to 31 pages, 109 sections, and 557 usage examples; re-run `wikitool docs import <Extension>` or `docs update` after upgrading to rehydrate. Redirect-resolved duplicate pages no longer abort an import, and talk-page and archive titles are filtered out of documentation corpora.

### Changed

- The bundled contextmink pin is now `0.6.0`, bringing the release pack up to the current transcript-guard surface (`dirs`, `outline`, `json-select`, `capture`/`run`, and hook-guard helpers) that the ai-pack guidance already teaches.
- Contextmink release staging is now source-owned by wikitool: public release builds compile the vendored `vendor/contextmink` source for each release target, while `--contextmink-dist` remains only an explicit prebuilt-pack override. The release validator rejects platform/binary/bridge mismatches before packaging.
- Every FTS MATCH expression built from arbitrary query text now goes through one shared sanitizing builder: multi-word topics match as all-tokens-as-prefixes with the exact phrase as an OR branch instead of an inert exact phrase, and a double quote in a topic can no longer produce a SQL syntax error (previously it could fail `article-start` outright).
- The consensus section skeleton in `article-start` follows the document order of the closest comparable pages instead of alphabetizing headings.
- Misleading article-start fields renamed to what they are: `EvidenceRef.score` is now `token_estimate` (it was a size, not a rank), `candidate_facts` is now `comparable_page_excerpts` (verbatim prose from other pages, not subject facts), `external_sources_shortlist` is now `citation_template_families` (template type labels, not followable sources), and the subject lane's `summary` is now `top_local_excerpt`.
- The article-start brief drops its `counts` block (it restated the lengths of the arrays beside it), and the pack no longer computes the ~16-query inventory sweep or media summaries that no command ever surfaced.
- Retrieval labels stop overselling: the page-relatedness prior built from a page's structural vocabulary is now called a term profile (`term-profile` retrieval mode and related-page source) instead of "semantic".

### Removed

- Schema autophagy, each item verified against actual read sites: four write-only FTS tables (template examples, module invocations, page references, page media) that were rebuilt on every index pass but never queried; the write-only `docs_links` table and its parser lane; the 14 JSON-list columns on the page term-profile table that duplicated seven relational tables and were never deserialized; `sync_snapshots.content_hash` and `synced_at_unix` (written, never read); the empty `src/index/` module directory. Table renames: `indexed_page_semantics` -> `indexed_page_term_profiles`, `indexed_authoring_contracts` -> `authoring_contracts` (it is catalog-owned, not a page-index child), `knowledge_artifacts` -> `runtime_artifacts` (a cross-module runtime KV store). Existing databases re-bootstrap automatically via the schema fingerprint; run `wikitool knowledge warm` (or `workflow session-refresh`) once after upgrading to repopulate the index.

## [0.5.0] - 2026-07-02

This release is a measured performance pass plus write-API completion. A new benchmark harness recorded a baseline for the heavy CLI lanes, and every optimization shipped here is justified by its before/after on that table; the headline is full-corpus `article lint` dropping from minutes to about a second.

### Added

- Release bundles ship the contextmink transcript guard in a `contextmink/` directory: the pinned release binary (plus `contextmink-bridge.exe` on Windows), instruction templates, and setup docs. contextmink stays a separate binary; nothing routes through wikitool. The pin lives in `config/contextmink.version` and bundles are fetched from contextmink's own GitHub releases at build time.
- `upload`, `purge`, and `move` API commands; `move` preflights the bot account's rights and supports `--no-redirect`, `--move-talk`, `--move-subpages`, and `--dry-run`.
- `protect` and `undelete` API commands with the same bot-rights preflight and `--dry-run` support, completing the write-API family (`watch` was considered and skipped: no wikitool workflow consumes watchlists).
- `wiki cargo count <table>` counts rows in a live Cargo extension table, giving `cargo_count_rows` its CLI surface under a `wiki cargo` group that later Cargo inspection commands can grow into.
- `--format json` on `knowledge inspect stats`, `knowledge inspect orphans`, `knowledge inspect empty-categories`, and `docs list`, closing the agent-facing JSON gaps those siblings already covered.
- `testbench/perf_bench.sh`: a performance harness that times end-to-end CLI scenarios (sync, lint, knowledge, refresh) against a disposable project copy and prints a baseline table; optimizations are held to before/after numbers on it.

### Performance

All numbers measured on the Remilia Wiki corpus (~676 pages, Windows) with `testbench/perf_bench.sh`; medians of repeated runs.

- Batch `article lint` (and the `review` gate's changed-article lint) loads project lint resources once per invocation instead of once per file: full-corpus lint of 395 articles drops from ~275s to ~1.5s with byte-identical findings.
- Database schema DDL and column validation now run once per database, stamped via `PRAGMA user_version`, instead of on every connection open and every sync ledger/snapshot upsert: `status --modified` drops from ~0.76s to ~0.47s and `diff` from ~0.44s to ~0.25s.
- The knowledge index rebuild is skipped when the scanned corpus is byte-identical to the current-generation index (any content, scan-set, or generation change still forces a full rebuild): `knowledge warm` on an unchanged corpus drops from ~1.6s to ~0.6s, and `workflow session-refresh` no longer pays a second full rebuild after pull (~6.1s to ~5.0s warm).
- The disposable runtime database uses `synchronous=NORMAL` under WAL, trimming per-commit fsync stalls on bulk index writes.
- `article fix` no longer reloads lint resources for its post-fix verification pass.

### Changed

- `knowledge build` and `knowledge warm` reports gain an `unchanged` field marking rebuilds that were skipped because the corpus already matched the index.
- Release notes moved from RELEASE_LOG.md to this CHANGELOG.

## [0.4.0] - 2026-06-05

This release adds a structured human-in-loop authoring lane. Wikitool now bundles a `/knowledge-interview` agent skill (for both Claude and Codex; other agents may need to adapt the packaged guidance) and a playbook for shaping an article's intent, scope, and angle before drafting, plus a `knowledge interview` command family that keeps a small ledger of what the interview turned up and what still needs explication or sourcing.

The interview is optional and conversational, not a step you clear before writing, but for real article work it is the normal move after the article-start scout, unless the user opts out or the task is mechanical. 
Its purpose is direction as much as fact intake: even a well-documented subject can need the editor's sense of what belongs on Remilia Wiki, what should be foregrounded, which sources or artifacts matter, and what should not be overstated. 
What the editor says is treated as reasonable truth once it passes a normal editorial check, rather than something that must be laundered through outside secondary coverage before it can be trusted at all. 
Cite a real source when one exists, especially for external, contested, or primary-record claims. 

Unlike the citation standards upheld by e.g. Wikipedia, for Remilia Wiki it is regularly the case that article content is rightfully its own primary source.
While this is not uncommon for subcultural and gaming-related MediaWikis either, it does mean that agent assistance is still beholden to the knowledge you personally provide it with; the interview skill exists to nudge you towards productive knowledge transfer upon the agent, but an article cannot write itself. Don't be lazy!

### New

- `knowledge interview` starts a brief, logs open questions and dead-end sources as open items, resolves them as they close, and validates or audits the ledger. A finished brief feeds `knowledge article-start` and `review` through `--brief-path`.
- The `/knowledge-interview` skill and `interview_playbook.md`, bundled for Claude and Codex.

### Improvements

- The article quality banner now reads as an editorial review state (`unverified`, `wip`, `verified`) rather than an AI-authorship label, and lint leaves an intentional state alone.
- Headings stop tripping over names. e.g. "Place in the Radbro Webring" is left alone because the tool recognizes Radbro and Webring as proper nouns, not Title Case slips.
- Template checks stop flagging parameters a template is visibly used with on real pages.

### Fixes

- Sentence-case heading suggestions keep proper nouns capitalized instead of lowercasing them.
- A page no longer shows as changed over a trailing-newline-only difference during sync.

## [0.3.1] - 2026-05-29

A follow-up to 0.3.0 that makes the wiki target durable and explicit, removes ambiguous overrides and vestigial flags, and tightens the agent-facing command contract. 0.3.0 reorganized the public surface; 0.3.1 makes the tool behave the way that surface implies.

### Breaking changes

- Bare `WIKI_*` environment variables are no longer read. Use `WIKITOOL_WIKI_URL`, `WIKITOOL_WIKI_API_URL`, `WIKITOOL_USER_AGENT`, `WIKITOOL_ARTICLE_PATH`, `WIKITOOL_BOT_USER`, and `WIKITOOL_BOT_PASS`.
- The default docs profile is renamed from `remilia-mw-1.44` to `remilia-wiki`, so the profile no longer pins a MediaWiki version.
- The `knowledge pack` command and the `knowledge article-start` raw-pack flags are removed. Use `knowledge article-start`, `knowledge contracts`, and `knowledge inspect` directly.
- The `--profile` flag is removed from `article lint`, `article fix`, and `review`. It only ever accepted `remilia`; the lint profile is now applied automatically and still reported as `profile_id` on each report.

### Improvements

- `wikitool config show` reports the resolved wiki target with the source of each value (env, config, default, or derived from the API URL) and notes which environment variables apply.
- `wikitool init` writes the Remilia Wiki target by default while runtime resolution stays env > config; `init --no-network` skips namespace discovery for offline bootstrap.
- `delete` gains `--format json`, so every command now has a structured output mode.
- Docs fetches against mediawiki.org send a User-Agent with a project contact URL and honor the upstream `Retry-After` header. Wikimedia began phasing in API rate limits in 2026, and a bare agent is more likely to be throttled.

### Fixes

- `knowledge article-start --view full` no longer emits a `raw_pack_ref` field pointing at the removed `knowledge pack` command.
- `validate --format json` reports findings in the JSON status instead of through a non-zero exit code on expected validation failures.
- Live wiki search snippets decode HTML entities before they reach JSON output.

### Upgrade

- Because the docs profile was renamed, docs imported under `remilia-mw-1.44` are no longer found under `remilia-wiki`. Run `wikitool knowledge warm --docs-profile remilia-wiki --docs-mode missing` (or `wikitool workflow session-refresh`) once to re-hydrate. Fresh installs need no action.

## [0.3.0] - 2026-05-28

A consolidation release. No database reset required — 0.2.0 runtime state carries forward.

0.2.0 introduced the knowledge layer; 0.3.0 builds the workflow around it and sharpens the agent-facing surface.

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

## [0.2.0] - 2026-03-18

Breaking release that replaces the retrieval layer with a purpose-built knowledge system for AI-assisted authoring.
Just delete your old wikitool installation ;d

### What changed

The core idea: wikitool's local index should give an AI agent everything it needs to write a good wiki article in one call. 0.1.0 had the raw materials — page chunks, template data, link graphs — but left the agent to assemble them. 0.2.0 introduces `knowledge article-start`, which interprets those materials into an opinionated authoring brief: which sections comparable articles use, which templates and categories apply, what type of subject this is, and where the evidence gaps are.

### Breaking changes

- **Database reset required.** The knowledge index schema is incompatible with 0.1.0. Delete `.wikitool/data/wikitool.db` and rebuild with `wikitool pull --full --all && wikitool knowledge warm --docs-profile remilia-wiki --docs-mode missing`.
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

## [0.1.0] - 2026-02-21

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
