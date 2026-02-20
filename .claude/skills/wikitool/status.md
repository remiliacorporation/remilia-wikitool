---
name: status
description: Inspect runtime and local state status.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [options]
---

# /wikitool status

Thin wrapper for:

```bash
wikitool status $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- status $ARGUMENTS
```

Validate flags via:

1. `wikitool status --help`
2. `docs/wikitool/reference.md`
