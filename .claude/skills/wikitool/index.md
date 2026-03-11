---
name: knowledge-inspect
description: Run `knowledge inspect` stats/backlinks/templates/orphans workflows. Use `knowledge build` for rebuilds.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <subcommand> [options]
---

# /wikitool knowledge inspect

Thin wrapper for:

```bash
wikitool knowledge inspect $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- knowledge inspect $ARGUMENTS
```

Validate flags via:

1. `wikitool knowledge inspect --help`
2. `docs/wikitool/reference.md`
