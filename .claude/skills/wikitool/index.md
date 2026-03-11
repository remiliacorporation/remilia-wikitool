---
name: index
description: Run index stats/backlinks/templates/orphans workflows. Use `knowledge build` for rebuilds.
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
