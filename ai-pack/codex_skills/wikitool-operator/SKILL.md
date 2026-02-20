---
name: wikitool-operator
description: Operate the Rust wikitool CLI for wiki editing sync, docs, import, and inspection workflows with dry-run guardrails and canonical CLI help alignment.
---

# Skill: wikitool-operator

Keep this skill as a thin overlay.
Canonical truth is `CLAUDE.md`, runbooks (`SETUP.md`, `docs/wikitool/*`), and live CLI help.

## Canonical lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

Do not introduce flags or command shapes that only exist in this skill.

## Safe write sequence

```bash
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

If dry-run is correct:

```bash
wikitool push --summary "Summary"
```

## Preflight and diagnostics

```bash
wikitool status
wikitool db stats
wikitool index stats
wikitool docs list --outdated
```

## Editing workflows

```bash
wikitool pull --full --all
wikitool context "Template:Infobox person"
wikitool search "Category:"
wikitool docs search "extension feature"
wikitool docs import --installed
```

## Safety constraints

1. Never skip dry-run before write push.
2. Do not use `--force` without explicit user approval.
3. For delete flows, require `--reason` and prefer `--dry-run` first.
4. Treat infrastructure/release operations as out of scope unless explicitly requested.
5. Run `db migrate` to apply pending schema migrations when prompted.
