# Wikitool Guide

Rust CLI that synchronizes MediaWiki content with local files and provides wiki-aware authoring retrieval, draft lint/remediation, docs ingestion, and inspection utilities.

For command flags: `wikitool <command> --help` or `reference.md`.

## Setup

```bash
wikitool init --templates
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
```

## How it works

- Pull/push use the MediaWiki API. Local state lives in SQLite under `.wikitool/data/wikitool.db`.
- The DB is disposable — delete it and repull/rebuild any time.
- Authoring retrieval uses semantic page profiles, template implementation bundles, normalized source authorities, and bridged MediaWiki docs to narrow context for agents.
- `knowledge article-start` is the interpreted authoring brief. `knowledge pack` is the raw substrate behind it.
- `article lint` / `article fix` are profile-aware. `validate` is the lower-level index integrity check. `module lint` is the Lua/module lane.
- `article lint` / `article fix` accept repeated `--title`, repeated `--path`, `--titles-file`, and `--changed` for batch work.
- Push flows require `--dry-run` first. Dry-run is the remote-aware preflight. `--force` requires explicit user approval.

## Authoring workflow

```bash
wikitool knowledge article-start "Topic" --format json
wikitool research search "Topic" --format json
wikitool research fetch "URL" --format rendered-html --output json
wikitool templates show "Template:Infobox person"
wikitool templates examples "Template:Infobox person" --limit 2
wikitool wiki profile show --format json
# write the article
wikitool article lint wiki_content/Main/Title.wiki --format json
wikitool article fix wiki_content/Main/Title.wiki --apply safe
wikitool knowledge inspect references summary --title "Title" --format json
wikitool knowledge inspect references duplicates --title "Title" --format json
wikitool validate
```

## Sync

```bash
wikitool pull                          # latest content
wikitool pull --full --all             # full refresh
wikitool pull --templates              # templates only
wikitool status                        # sync-aware status summary
wikitool status --modified --format json
wikitool status --conflicts --title "Title"
wikitool diff                          # review change set
wikitool diff --content --title "Title"
wikitool push --dry-run --summary "x"  # remote-safe preflight
wikitool push --dry-run --title "Title" --summary "x"
wikitool push --summary "x"            # actual push
wikitool delete "Title" --reason "x" --dry-run
```

## Knowledge and retrieval

```bash
wikitool knowledge build                # content index only
wikitool knowledge warm --docs-profile remilia-mw-1.44  # index + docs
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
wikitool knowledge article-start "Topic" --format json
wikitool knowledge pack "Topic" --format json
wikitool knowledge inspect stats
wikitool knowledge inspect chunks "Title" --query "aspect" --limit 6 --token-budget 480
wikitool knowledge inspect chunks --across-pages --query "topic" --max-pages 8 --token-budget 1200 --format json --diversify
wikitool knowledge inspect references summary --format json
wikitool knowledge inspect references list --title "Title" --domain remilia.org --format json
wikitool knowledge inspect references duplicates --all --identifier-key doi --format json
wikitool knowledge inspect backlinks "Title"
wikitool knowledge inspect orphans
wikitool knowledge inspect empty-categories
```

## Research

```bash
wikitool research search "topic" --format json
wikitool research fetch "URL" --format rendered-html --output json
wikitool fetch "URL" --format wikitext --save
wikitool export "URL" --subpages --combined
```

`fetch` and `export` accept MediaWiki short URLs, `index.php?title=` URLs, and subdirectory installs.

## Editor integration

```bash
wikitool lsp generate-config
wikitool lsp status
wikitool lsp info
```

## Docs

```bash
wikitool docs import-profile remilia-mw-1.44
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs search "topic" --profile remilia-mw-1.44
wikitool docs context "Extension" --profile remilia-mw-1.44 --format json
wikitool docs symbols "$wg" --profile remilia-mw-1.44
wikitool docs list
wikitool docs update
```

## Templates and profile

```bash
wikitool templates show "Template:Cite web"
wikitool templates examples "Template:Cite web" --limit 2
wikitool templates catalog build
wikitool wiki capabilities sync --format json
wikitool wiki profile sync --format json
wikitool wiki profile show --format json
wikitool wiki rules show --format json
```

## Diagnostics

```bash
wikitool status
wikitool lsp status
wikitool lsp info
wikitool db stats
wikitool seo inspect "Page"
wikitool net inspect "Page" --limit 25
wikitool module lint --format text
```

## Release packaging

These maintainer commands are available from source-checkout builds with the maintainer surface
enabled. Packaged end-user binaries do not include them, and they remain hidden from default
`wikitool --help` output and the generated reference.

```bash
wikitool release build-matrix --targets x86_64-pc-windows-msvc,x86_64-unknown-linux-gnu,x86_64-apple-darwin
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --unversioned-names
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --host-project-root <PATH>
```

## Troubleshooting

If local state drifts or schema changes:

```bash
rm .wikitool/data/wikitool.db        # or: wikitool db reset --yes
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44
```

If push/delete writes fail, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.

Starting in v0.2.0, pre-manifest databases are treated as incompatible. The supported path is reset, repull, rebuild.
