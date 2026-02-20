---
name: search-external
description: Search remote wiki content via API.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [query] [options]
---

# /wikitool search-external

Thin wrapper for:

```bash
wikitool search-external $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- search-external $ARGUMENTS
```

Validate flags via:

1. `wikitool search-external --help`
2. `docs/wikitool/reference.md`
