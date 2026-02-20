---
name: index
description: Run index rebuild/stats/backlinks/orphans workflows.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <subcommand> [options]
---

# /wikitool index

Thin wrapper for:

```bash
wikitool index $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- index $ARGUMENTS
```

Validate flags via:

1. `wikitool index --help`
2. `docs/wikitool/reference.md`
