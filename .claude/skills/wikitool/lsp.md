---
name: lsp
description: Generate and inspect LSP configuration for editors
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: lsp:generate-config | lsp:status | lsp:info
---

# /wikitool lsp - Editor Integration

Generate and inspect LSP configuration for wikitext and Lua editing.

## Reference

See `docs/wikitool/reference.md` for full commands and flags.

## Examples

```bash
/wikitool lsp:generate-config
/wikitool lsp:status
/wikitool lsp:info
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool $ARGUMENTS
```
