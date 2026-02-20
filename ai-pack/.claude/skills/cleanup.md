# /cleanup - Cleanup and Auditing

Fix style drift, broken structure, and category/link hygiene.

## Audit workflow

```bash
wikitool pull
wikitool validate
wikitool index orphans
wikitool index prune-categories
wikitool diff
```

## Link/category checks

```bash
wikitool search "Category:"
wikitool index backlinks "Article Title"
wikitool index chunks --across-pages --query "topic" --format json --diversify
```

## Push gate

```bash
wikitool push --dry-run --summary "Cleanup: <scope>"
```

Only run non-dry-run push when explicitly requested.
