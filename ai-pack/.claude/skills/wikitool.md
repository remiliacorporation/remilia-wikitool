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
3. Treat `db migrate` as intentionally unsupported during cutover.
