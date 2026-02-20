---
name: search
description: Search local indexed content.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [query] [options]
---

# /wikitool search

Thin wrapper for:

```bash
wikitool search $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- search $ARGUMENTS
```

Validate flags via:

1. `wikitool search --help`
2. `docs/wikitool/reference.md`
