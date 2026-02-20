---
name: import
description: Run cargo import workflow into wiki pages.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <cargo ...> [options]
---

# /wikitool import

Thin wrapper for:

```bash
wikitool import $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- import $ARGUMENTS
```

Validate flags via:

1. `wikitool import --help`
2. `docs/wikitool/reference.md`
