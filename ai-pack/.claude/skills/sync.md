# /sync - Wiki Sync Operations

Thin wrapper for sync and refresh workflows.
Use `wikitool` for pulling, diffing, validating, and guarded pushes. Do not force unrelated authoring or reasoning through it.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Setup and refresh

```bash
wikitool init --templates
wikitool pull --full --all
wikitool workflow bootstrap
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
