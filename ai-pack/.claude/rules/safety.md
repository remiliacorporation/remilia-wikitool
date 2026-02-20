# Safety Rules (AI Pack)

These rules are always enforced for wiki editing tasks.

## Local vs live wiki

1. Treat local files as the editing workspace.
2. Read-only API calls to `https://wiki.remilia.org/w/api.php` are safe.
3. Never perform direct live-wiki writes outside `wikitool` workflows.

## Push safety

Before any write push:

1. `wikitool diff`
2. `wikitool validate`
3. `wikitool push --dry-run --summary "Summary"`

Only after dry-run is verified:

1. `wikitool push --summary "Summary"`

## Force and delete safeguards

1. Never use `--force` without explicit user approval.
2. For delete workflows, require explicit scope confirmation and reason text.
3. Prefer `--dry-run` for destructive operations.

## Scope safety

This pack is editing-focused. Do not run infrastructure/deployment/release operations unless explicitly requested.
