---
name: lsp
description: Generate or refresh LSP parser config.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [options]
---

# /wikitool lsp

Thin wrapper for:

```bash
wikitool lsp:generate-config $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- lsp:generate-config $ARGUMENTS
```

Validate flags via:

1. `wikitool lsp:generate-config --help`
2. `docs/wikitool/reference.md`
