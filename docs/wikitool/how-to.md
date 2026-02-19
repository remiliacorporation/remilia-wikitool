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
wikitool pull --category "Category:Remilia"
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
wikitool index backlinks "Main Page"
wikitool index orphans
wikitool index prune-categories
```

## Inspection workflows

```bash
wikitool lint --format text
wikitool seo inspect "Main Page"
wikitool net inspect "Main Page" --limit 25
wikitool perf lighthouse "Main Page" --output html
```

## Runtime checks

```bash
wikitool status
wikitool db stats
wikitool db sync
```

`db migrate` is intentionally disabled under no-migration cutover policy.

## Troubleshooting

If local state drifts or schema changes:

1. delete `.wikitool/data/wikitool.db`
2. run `wikitool pull --full --all`

If push/delete writes fail, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
