---
name: seo
description: Run SEO/net/perf inspection workflows.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: inspect [target] [options]
---

# /wikitool seo

Thin wrapper for:

```bash
wikitool seo inspect $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- seo inspect $ARGUMENTS
```

Validate flags via:

1. `wikitool seo inspect --help`
2. `docs/wikitool/reference.md`
