---
name: fetch
description: Fetch external pages for local reference.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [url] [options]
---

# /wikitool fetch

Thin wrapper for:

```bash
wikitool fetch $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- fetch $ARGUMENTS
```

Validate flags via:

1. `wikitool fetch --help`
2. `docs/wikitool/reference.md`
