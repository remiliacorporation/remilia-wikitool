# /mw-fetch - External Wiki Fetch/Export

Fetch and export external wiki docs and pages.

## Commands

```bash
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks/BeforePageDisplay"
wikitool fetch "https://en.wikipedia.org/wiki/NFT" --format html --save
wikitool export "https://wowdev.wiki/M2" --subpages --combined
wikitool search-external "MediaWiki Cargo"
```

## Docs import/search

```bash
wikitool docs import --installed
wikitool docs import Cargo Scribunto
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs search "Cargo" --tier extension
```
