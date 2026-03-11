# /cleanup - Cleanup and Auditing

Fix style drift, broken structure, and category/link hygiene.

## Audit workflow

```bash
wikitool pull
wikitool validate
wikitool knowledge inspect orphans
wikitool knowledge inspect empty-categories
wikitool diff
```

## Link/category checks

```bash
wikitool search "Category:"
wikitool knowledge inspect backlinks "Article Title"
wikitool knowledge inspect chunks --across-pages --query "topic" --format json --diversify
```

## Push gate

```bash
wikitool push --dry-run --summary "Cleanup: <scope>"
```

Only run non-dry-run push when explicitly requested.
