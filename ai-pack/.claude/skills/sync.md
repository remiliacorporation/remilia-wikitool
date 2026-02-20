# /sync - Wiki Sync Operations

Use safe pull/diff/validate/dry-run push flow for content sync.

## Setup and pull

```bash
wikitool init --templates
wikitool pull --full --all
```

## Daily sequence

```bash
wikitool pull
# edit local files
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

Only run non-dry-run push when explicitly requested.

## Namespace pulls

```bash
wikitool pull --templates
wikitool pull --categories
wikitool pull --all
```
