# /wikitool - CLI Gateway

Thin wrapper for deciding when to use `wikitool`.
Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Reach for `wikitool` when you need wiki-aware retrieval, MediaWiki-aware fetch/export, template/profile lookup, article lint/fix/validate, or guarded sync/push.

## Lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Common lanes

```bash
wikitool knowledge status --docs-profile remilia-mw-1.44
wikitool knowledge article-start "Topic" --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool article lint wiki_content/Main/<Title>.wiki --format json
wikitool article fix wiki_content/Main/<Title>.wiki --apply safe
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

## Diagnostics

```bash
wikitool status
wikitool knowledge status --docs-profile remilia-mw-1.44
wikitool db stats
wikitool knowledge inspect stats
wikitool docs list --outdated
```

## Safety

1. Never skip dry-run before write push.
2. Do not use `--force` without explicit user approval.
3. Treat the local DB as disposable; use `db reset` or delete `.wikitool/data/wikitool.db` instead of preserving old schema state.

## Retrieval guidance

1. Use `knowledge warm`, `knowledge status`, `knowledge article-start`, `research search`, `research fetch`, and `knowledge inspect chunks --across-pages` for AI-facing article retrieval.
2. The DB stores semantic retrieval signals, normalized source authorities, identifier rows, template implementation bundles, module invocation patterns, pinned docs corpora, and source metadata, not opaque quality scores.
3. Use `knowledge pack` only when the deeper raw authoring substrate is needed behind `article-start`.
