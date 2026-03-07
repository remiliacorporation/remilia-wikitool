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
wikitool docs import-profile remilia-mw-1.44
wikitool docs import-profile mw-1.44-authoring --extension Cargo --extension Scribunto
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs search "Cargo" --profile remilia-mw-1.44 --tier extension
wikitool docs context "parser function" --profile remilia-mw-1.44 --format json
```
