# /wikitool - CLI Gateway

Run `wikitool` command workflows while keeping docs/help as source of truth.

## Lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Core operator sequence

```bash
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

## Diagnostics

```bash
wikitool status
wikitool db stats
wikitool index stats
wikitool docs list --outdated
```

## Safety

1. Never skip dry-run before write push.
2. Do not use `--force` without explicit user approval.
3. Treat the local DB as disposable; use `db reset` or delete `.wikitool/data/wikitool.db` instead of preserving old schema state.

## Retrieval guidance

1. Use `workflow authoring-pack` and `index chunks --across-pages` for AI-facing article retrieval.
2. The DB stores semantic retrieval signals and source metadata, not opaque quality scores.
