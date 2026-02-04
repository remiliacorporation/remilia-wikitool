---
name: export
description: Export external wiki page to AI-friendly markdown
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*), Read, Write
argument-hint: <url> [options]
---

# /wikitool export - Export Wiki to Markdown

Export wiki pages to AI-friendly markdown format. Supports MediaWiki sites, custom wikis, and direct markdown URLs.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Default Output Directory

When `-o` is not specified, exports go to `wikitool_exports/` at the repo root.
Set `WIKITOOL_NO_DEFAULT_EXPORTS=1` to disable this behavior.

## Supported URL Types

- **MediaWiki sites**: `https://en.wikipedia.org/wiki/Page`
- **Custom wikis**: `https://wowdev.wiki/M2`
- **Direct markdown**: `https://example.com/docs/file.md`

## Examples

```bash
# Export single page
/wikitool export "https://en.wikipedia.org/wiki/Ethereum" -o ethereum.md

# Export page with all subpages to directory
/wikitool export "https://wowdev.wiki/M2" --subpages -o exports/M2/

# Export direct markdown file
/wikitool export "https://example.com/docs/format.md" -o format.md
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool export $ARGUMENTS
```


