---
name: search-external
description: Search Wikipedia, MediaWiki.org, or custom wiki domains
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: "<query>" [options]
---

# /wikitool search-external - Search External Wikis

Search Wikipedia, MediaWiki.org, or other MediaWiki sites.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Known Wikis (--wiki)

- `wikipedia` (default) - English Wikipedia
- `mediawiki` - MediaWiki.org documentation
- `commons` - Wikimedia Commons
- `wikidata` - Wikidata

Use `--lang` to change the Wikipedia language (default is `en`).

## Custom Domains (--domain)

For wikis not in the known list, use `--domain` with the wiki's base URL.
Use `--api-url` if the API endpoint is non-standard.

## Examples

```bash
/wikitool search-external "Ethereum blockchain"
/wikitool search-external "Extension:Cargo" --wiki mediawiki
/wikitool search-external "NFT" --wiki wikidata
/wikitool search-external "M2 format" --domain wowdev.wiki
/wikitool search-external "Ethereum" --wiki wikipedia --lang de
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool search-external $ARGUMENTS
```
