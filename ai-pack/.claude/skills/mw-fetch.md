# /mw-fetch - External Wiki Fetch/Export

Thin wrapper for raw fetch/export and pinned docs workflows.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Raw and readable fetch lanes

```bash
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks/BeforePageDisplay"
wikitool fetch "https://en.wikipedia.org/wiki/NFT" --format html --save
wikitool export "https://wowdev.wiki/M2" --subpages --combined
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool search-external "MediaWiki Cargo"
```

Use `research fetch` for readable evidence extraction.
Use `fetch` and `export` for raw/reference-oriented capture.

## Docs import/search

```bash
wikitool docs import-profile remilia-mw-1.44
wikitool docs import-profile mw-1.44-authoring --extension Cargo --extension Scribunto
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs search "Cargo" --profile remilia-mw-1.44 --tier extension
wikitool docs context "parser function" --profile remilia-mw-1.44 --format json
```
