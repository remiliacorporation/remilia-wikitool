---
name: fetch
description: Fetch raw wikitext from external wiki (not converted to markdown)
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: <url> [options]
---

# /wikitool fetch - Fetch Raw Wikitext

Fetch raw wikitext from external wiki pages. Unlike `export`, this returns unconverted wikitext.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## When to Use

- **Use `fetch`** when you need raw wikitext syntax (for templates, studying markup)
- **Use `export`** when you need readable markdown (for AI context, documentation)

## Examples

```bash
# Fetch raw wikitext
/wikitool fetch "https://en.wikipedia.org/wiki/Ethereum"

# Fetch template source
/wikitool fetch "https://en.wikipedia.org/wiki/Template:Infobox_cryptocurrency"
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool fetch $ARGUMENTS
```


