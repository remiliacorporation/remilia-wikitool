---
name: db
description: Run database maintenance and status commands.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <stats|sync|migrate> [options]
---

# /wikitool db

Thin wrapper for:

```bash
wikitool db $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- db $ARGUMENTS
```

Validate flags via:

1. `wikitool db --help`
2. `docs/wikitool/reference.md`
