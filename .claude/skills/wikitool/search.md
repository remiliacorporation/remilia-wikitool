---
name: search
description: Full-text search local wiki content
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: "<query>" [options]
---

# /wikitool search - Search Local Content

Full-text search across local wiki content in `wiki_content/`.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool search "Milady Maker"
/wikitool search "Remilia Corporation"
/wikitool search "NFT collection"
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool search $ARGUMENTS
```


