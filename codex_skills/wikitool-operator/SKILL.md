# Skill: wikitool-operator

Operate the Rust `wikitool` binary safely for pull/index/docs/import/push flows.

## Preconditions

1. Run commands from `custom/wikitool`.
2. Ensure runtime exists (`wikitool init --templates` if needed).
3. Use project-root `.env` for bot credentials when write operations are required.

## Core sequence

```bash
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

If dry-run output is correct:

```bash
wikitool push --summary "Summary"
```

## Fast diagnostics

```bash
wikitool status
wikitool db stats
wikitool index stats
wikitool docs list --outdated
```

## Safety constraints

1. Never skip dry-run before push.
2. Do not use `--force` without explicit user approval.
3. For deletions, require `--reason` and prefer `--dry-run` first.
4. Treat `db migrate` as unsupported during cutover policy.
