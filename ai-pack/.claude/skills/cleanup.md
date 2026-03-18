# /cleanup - Cleanup and Auditing

Thin wrapper for cleanup passes.
Use normal editing for the draft itself. Use `wikitool` to surface wiki-specific lint, validation, link/category audits, and push guards.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Audit workflow

```bash
wikitool pull
wikitool article lint wiki_content/Main/<Title>.wiki --format json
wikitool article fix wiki_content/Main/<Title>.wiki --apply safe
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
