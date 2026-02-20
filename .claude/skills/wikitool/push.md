---
name: push
description: Push local edits (dry-run first, then write).
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: --dry-run --summary "..." [options]
---

# /wikitool push

Thin wrapper for:

```bash
wikitool push $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- push $ARGUMENTS
```

Validate flags via:

1. `wikitool push --help`
2. `docs/wikitool/reference.md`
