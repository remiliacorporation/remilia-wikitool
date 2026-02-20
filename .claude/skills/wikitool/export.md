---
name: export
description: Export external references to markdown or wikitext.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [url] [options]
---

# /wikitool export

Thin wrapper for:

```bash
wikitool export $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- export $ARGUMENTS
```

Validate flags via:

1. `wikitool export --help`
2. `docs/wikitool/reference.md`
