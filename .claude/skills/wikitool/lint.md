---
name: lint
description: Run Lua/module lint checks.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [title] [options]
---

# /wikitool lint

Thin wrapper for:

```bash
wikitool lint $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- lint $ARGUMENTS
```

Validate flags via:

1. `wikitool lint --help`
2. `docs/wikitool/reference.md`
