# Wikitool How-To

Task-focused recipes for common workflows.

## First-time setup

```bash
wikitool init --templates
wikitool pull --full --all
```

## Default authoring workflow

```bash
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge article-start "Remilia Corporation" --format json
wikitool research search "Remilia Corporation" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
wikitool article fix wiki_content/Main/Remilia_Corporation.wiki --apply safe
wikitool validate
```

Use `knowledge pack` after `article-start` only when you need the deeper raw retrieval payload.

## Pull latest content

```bash
wikitool pull
wikitool pull --full
wikitool pull --full --all
```

## Pull by scope

```bash
wikitool pull --templates
wikitool pull --categories
wikitool pull --category "Category:Example"
```

## Review local changes

```bash
wikitool diff
wikitool status --modified
```

## Validate content

```bash
wikitool validate
```

## Push changes safely

```bash
wikitool push --dry-run --summary "Edit summary"
wikitool push --summary "Edit summary"
```

## Delete a page (local + optional remote)

```bash
wikitool delete "Page Title" --reason "Cleanup" --dry-run
wikitool delete "Page Title" --reason "Cleanup"
```

Remote delete is attempted only when write credentials are configured.

## Docs workflows

```bash
wikitool docs import-profile remilia-mw-1.44
wikitool docs import-profile mw-1.44-authoring --extension Scribunto --extension TemplateStyles
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs list
wikitool docs search "parser function" --profile remilia-mw-1.44
wikitool docs context "TemplateStyles" --profile remilia-mw-1.44 --format json
wikitool docs symbols "$wg" --profile remilia-mw-1.44
wikitool docs update
```

`remilia-mw-1.44` attempts installed-extension discovery when the configured wiki API allows it. If that discovery step is unavailable, the pinned core corpus still imports and can be used by `knowledge warm` / `knowledge article-start`.

## Knowledge command chooser

- `knowledge build` for a content-only rebuild
- `knowledge warm --docs-profile ...` for content indexing plus pinned docs hydration
- `knowledge status --docs-profile ...` before depending on local authoring retrieval
- `knowledge article-start "Topic"` for the default interpreted authoring brief
- `research search` / `research fetch` for external source discovery and extraction
- `article lint` / `article fix` for the draft quality loop
- `knowledge pack "Topic"` for the deeper raw context bundle behind `article-start`
- `wiki profile sync|show` and `templates ...` for live capability/profile/template awareness
- `knowledge inspect ...` for low-level chunk/template/backlink/orphan inspection
- `context` and `search` for quick indexed lookups
- `docs ...` for direct docs administration and direct docs queries

## Fetch/export external sources

```bash
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks" --save
wikitool export "https://www.mediawiki.org/wiki/Manual:Hooks" --subpages --combined
```

## Cargo import

```bash
wikitool import cargo ./data.csv --table Items --mode upsert --write
```

## Knowledge Inspect Workflows

```bash
wikitool knowledge build
wikitool knowledge status
wikitool knowledge inspect stats
wikitool knowledge inspect chunks "Main Page" --query "infobox" --limit 6 --token-budget 480
wikitool knowledge inspect chunks --across-pages --query "foundational concepts" --max-pages 8 --limit 10 --token-budget 1200 --format json --diversify
wikitool knowledge inspect backlinks "Main Page"
wikitool knowledge inspect orphans
wikitool knowledge inspect empty-categories
```

Command chooser:

- Use `knowledge build` when you only need the local content index
- Use `knowledge warm` when authoring retrieval also needs pinned MediaWiki docs
- Use `knowledge status` to check readiness and degradations first
- Use `knowledge pack` for the AI-facing authoring bundle
- Use `knowledge inspect ...` for low-level chunk/template/backlink/orphan/empty-category inspection
- Use top-level `context` or `search` for quick indexed lookups against local wiki content
- Use `docs context` or `docs search` when you need pinned MediaWiki docs retrieval
- Use `docs ...` when you are managing or querying docs corpora directly

Legacy command mapping:

- `wikitool index stats` -> `wikitool knowledge inspect stats`
- `wikitool index chunks ...` -> `wikitool knowledge inspect chunks ...`
- `wikitool index backlinks ...` -> `wikitool knowledge inspect backlinks ...`
- `wikitool index templates ...` -> `wikitool knowledge inspect templates ...`
- `wikitool index orphans` -> `wikitool knowledge inspect orphans`
- `wikitool index prune-categories` -> `wikitool knowledge inspect empty-categories`

## Research workflows

```bash
wikitool research search "network spirituality remilia" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool research fetch "https://blog.rust-lang.org/2023/07/13/Rust-1.71.0/" --output json
```

## Wiki profile and template workflows

```bash
wikitool wiki capabilities sync --format json
wikitool wiki profile sync --format json
wikitool wiki rules show --format json
wikitool templates catalog build --format json
wikitool templates show "Template:Cite web"
wikitool templates examples "Template:Cite web" --limit 2
```

## Article lint and fix

```bash
wikitool article lint wiki_content/Main/Remilia_Corporation.wiki --format json
wikitool article fix wiki_content/Main/Remilia_Corporation.wiki --apply safe
wikitool validate
```

`article lint` is article-aware and profile-aware. `validate` remains the lower-level index integrity check.

## Advanced/raw authoring knowledge pack

Generate a token-budgeted local context pack for writing new articles or upgrading stubs:

```bash
wikitool knowledge pack "Example Topic" --format json
wikitool knowledge pack "Milady Maker" --stub-path wiki_content/Main/Milady_Draft.wiki --format json
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
```

The pack includes:
- related local pages
- suggested internal links/categories
- template usage summaries (topic-scoped + global baseline)
- template implementation references across `Template:`, `/doc`, helper pages, and `Module:`
- module invocation patterns gathered from local template/article usage
- bridged MediaWiki docs context from the pinned `remilia-mw-1.44` corpus when that profile is imported
- chunked cross-page context under a strict token budget
- stub diagnostics (existing links, missing links, templates already used)

Prefer `knowledge article-start` first. Use `knowledge pack` when you want the uncollapsed substrate that `article-start` interprets.

## Inspection workflows

```bash
wikitool lint --format text
wikitool seo inspect "Main Page"
wikitool net inspect "Main Page" --limit 25
wikitool perf lighthouse "Main Page" --output html
```

## Build release bundles (manual)

```bash
wikitool release build-matrix --targets x86_64-pc-windows-msvc,x86_64-unknown-linux-gnu,x86_64-apple-darwin
```

Default output names are versioned (`wikitool-vX.Y.Z-<target>.zip`).

For ephemeral CI-style names:

```bash
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --unversioned-names
```

To inject host project guardrails into packaged artifacts:

```bash
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --host-project-root <PATH>
```

## Community-parity smoke test

Goal: validate the same packaged experience a community editor gets.

1. Build one target bundle without host overlay:

```bash
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --artifact-version vlocal
```

2. Unzip `wikitool-vlocal-x86_64-unknown-linux-gnu.zip`.
3. Confirm package includes:
   - `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
   - `.claude/rules/`, `.claude/skills/`
   - `llm_instructions/`
   - `docs/wikitool/`
   - `codex_skills/`
4. Confirm no host overlay unless requested:
   - `CLAUDE.md` and `AGENTS.md` are identical
   - no `WIKITOOL_CLAUDE.md`
5. Run basic commands from the unpacked folder:
   - `wikitool --help`
   - `wikitool init --project-root <test-project> --templates`
   - `wikitool pull --project-root <test-project> --full --all`

## Runtime checks

```bash
wikitool status
wikitool db stats
wikitool knowledge status
wikitool knowledge inspect stats
bash testbench/acceptance_workflows.sh
```

## Troubleshooting

If local state drifts or schema changes:

1. delete `.wikitool/data/wikitool.db`
2. run `wikitool pull --full --all`
3. rebuild retrieval state with `wikitool knowledge build` or `wikitool knowledge warm --docs-profile remilia-mw-1.44`

If push/delete writes fail, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
