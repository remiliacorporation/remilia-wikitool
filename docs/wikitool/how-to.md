# Wikitool How-To

Task-focused recipes for common workflows.

## First-time setup

```bash
wikitool init --templates
wikitool pull --full --all
```

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
wikitool docs import --installed
wikitool docs import SemanticMediaWiki
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs list
wikitool docs search "parser function"
wikitool docs update
```

## Fetch/export external sources

```bash
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks" --save
wikitool export "https://www.mediawiki.org/wiki/Manual:Hooks" --subpages --combined
```

## Cargo import

```bash
wikitool import cargo ./data.csv --table Items --mode upsert --write
```

## Index workflows

```bash
wikitool index rebuild
wikitool index stats
wikitool index chunks "Main Page" --query "infobox" --limit 6 --token-budget 480
wikitool index chunks --across-pages --query "foundational concepts" --max-pages 8 --limit 10 --token-budget 1200 --format json --diversify
wikitool index backlinks "Main Page"
wikitool index orphans
wikitool index prune-categories
```

## AI authoring knowledge pack

Generate a token-budgeted local context pack for writing new articles or upgrading stubs:

```bash
wikitool workflow authoring-pack "Example Topic" --format json
wikitool workflow authoring-pack "Milady Maker" --stub-path wiki_content/Main/Milady_Draft.wiki --format json
```

The pack includes:
- related local pages
- suggested internal links/categories
- template usage summaries (topic-scoped + global baseline)
- chunked cross-page context under a strict token budget
- stub diagnostics (existing links, missing links, templates already used)

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
wikitool db sync
wikitool db migrate
```

## Troubleshooting

If local state drifts or schema changes:

1. delete `.wikitool/data/wikitool.db`
2. run `wikitool pull --full --all`

If push/delete writes fail, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
