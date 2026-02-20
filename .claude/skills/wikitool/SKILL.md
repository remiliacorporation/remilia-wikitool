---
name: wikitool
description: Run the Rust wikitool CLI for sync, indexing, docs, import, inspection, workflow, and release operations. Use when executing command-line workflows with dry-run guardrails and canonical CLI-help validation.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [command] [options]
---

# /wikitool - Rust CLI Gateway

Treat this skill as a thin overlay.
Canonical behavior is defined by runbooks and live CLI help.

## Usage

```text
/wikitool
/wikitool <command>
/wikitool <command> [args]
```

## Resolution

1. Prefer direct binary: `wikitool ...`.
2. If binary is unavailable, use `cargo run --quiet --package wikitool -- ...`.
3. Do not invent flags from memory; verify against help.

## Canonical lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Guardrails

1. Run `diff` before push/delete workflows.
2. Run `push --dry-run --summary "..."` before write push.
3. Do not use `--force` without explicit approval.
4. Treat `db migrate` as intentionally unsupported during cutover.

## Binary-native workflow helpers

```bash
wikitool workflow bootstrap
wikitool workflow full-refresh --yes
wikitool docs generate-reference
wikitool dev install-git-hooks
wikitool release build-ai-pack
wikitool release package
wikitool release build-matrix
```
