# /wikitool - Operator

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Reach for `wikitool` when you need wiki-grounded context, MediaWiki-aware fetch, template/profile lookup, article lint/fix, or guarded sync.

Do not route every step through `wikitool`. Do not invent flags; verify against `wikitool <command> --help` or `docs/wikitool/reference.md`.

## Retrieval principles

1. Local files are the human editing surface. SQLite is the AI retrieval layer.
2. `knowledge article-start` is the front door for authoring — it returns article type, infobox, categories, links, skeleton, evidence, and citation families in one call.
3. Use `knowledge pack` only when article-start is too collapsed and you need the raw substrate.
4. `templates show` and `templates examples` give parameter shapes and real invocations — use them instead of guessing template syntax.
5. `wiki profile show` exposes Remilia-specific infobox preferences, citation families, banned phrases, and lint rules.
6. When `remilia-mw-1.44` docs are imported, authoring retrieval bridges pinned MediaWiki docs with local template/module patterns; use that before falling back to generic web docs.
7. wikitool does not assign opaque quality scores — it stores inspectable source metadata, authority matches, and retrieval signals.

## Authoring lane

```text
1. knowledge article-start "Topic" --format json    → context, infobox, categories, skeleton
2. research search "Topic" --format json             → live wiki evidence (if needed)
3. research fetch "URL" --output json                → readable URL extraction (if needed)
4. templates show "Template:Name"                    → parameter surface
5. Write article to wiki_content/Main/<Title>.wiki
6. article lint <path> --format json                 → profile-aware lint
7. article fix <path> --apply safe                   → mechanical fixes
8. validate → diff → push --dry-run (only when push is requested)
```

## Command chooser

### Context and retrieval
| Need | Command |
|------|---------|
| Authoring brief | `knowledge article-start "Topic" --format json` |
| Raw authoring pack | `knowledge pack "Topic" --format json` |
| Cross-page chunks | `knowledge inspect chunks --across-pages --query "..." --max-pages 8 --token-budget 1200 --format json --diversify` |
| Single-page chunks | `knowledge inspect chunks "Title" --query "..." --limit 6 --token-budget 480` |
| Quick title search | `search "topic"` |
| Quick page context | `context "Title"` |
| Check readiness | `knowledge status --docs-profile remilia-mw-1.44 --format json` |

### Research and fetch
| Need | Command |
|------|---------|
| Wiki API search | `research search "Topic" --format json` |
| Readable URL fetch | `research fetch "URL" --format rendered-html --output json` |
| Raw wikitext fetch | `fetch "URL" --format wikitext` |
| Bulk page export | `export "URL" --subpages --combined` |
| External wiki search | `search-external "query"` |

### Templates and profile
| Need | Command |
|------|---------|
| Template parameters | `templates show "Template:Name"` |
| Usage examples | `templates examples "Template:Name" --limit 2` |
| Profile/citation rules | `wiki profile show --format json` |
| Category inventory | `search "Category:"` |

### Docs
| Need | Command |
|------|---------|
| Hydrate docs profile | `docs import-profile remilia-mw-1.44` |
| Search extension docs | `docs search "topic" --profile remilia-mw-1.44 --tier extension` |
| Extension context | `docs context "Extension" --profile remilia-mw-1.44 --format json` |

### Sync
| Need | Command |
|------|---------|
| Pull content | `pull`, `pull --full --all`, `pull --templates` |
| Review changes | `diff`, `status` |
| Validate integrity | `validate` |
| Safe push | `push --dry-run --summary "..."` then `push --summary "..."` |
| Delete page | `delete "Title" --reason "..." --dry-run` |

### Diagnostics
| Need | Command |
|------|---------|
| Runtime status | `status` |
| DB stats | `db stats` |
| Index stats | `knowledge inspect stats` |
| Orphan pages | `knowledge inspect orphans` |
| Empty categories | `knowledge inspect empty-categories` |
| Backlinks | `knowledge inspect backlinks "Title"` |
| SEO metadata | `seo inspect "Page"` |
| Network resources | `net inspect "Page" --limit 25` |

### Bootstrap (first time or after reset)
```bash
knowledge warm --docs-profile remilia-mw-1.44
wiki profile sync
knowledge status --docs-profile remilia-mw-1.44 --format json
```

## Safety

1. Never skip `--dry-run` before write push.
2. Do not use `--force` without explicit user approval.
3. For delete flows, require `--reason` and prefer `--dry-run` first.
4. The local DB is disposable — `db reset --yes` then repull and rebuild.
5. Infrastructure/release operations are out of scope unless explicitly requested.
