# /review - Content Gate

Validate, audit, and gate wiki content before push.
Use `wikitool` for deterministic wiki-aware checks. Editorial judgment is yours, not wikitool's.

## Gate sequence

```bash
wikitool article lint wiki_content/Main/<Title>.wiki --format json
wikitool validate
wikitool diff
```

## Fix loop

When lint reports issues:

```bash
wikitool article fix wiki_content/Main/<Title>.wiki --apply safe
wikitool article lint wiki_content/Main/<Title>.wiki --format json   # re-lint to verify
```

Fix what `--apply safe` cannot handle manually, then re-lint until clean.

## Audit signals

Use these for cleanup passes or broader content review:

| Need | Command |
|------|---------|
| Orphan pages (no backlinks) | `knowledge inspect orphans` |
| Empty categories | `knowledge inspect empty-categories` |
| What links to a page | `knowledge inspect backlinks "Title"` |
| Category inventory | `search "Category:"` |
| Template usage | `templates show "Template:Name"` |
| Profile lint rules | `wiki profile show --format json` |

## Push-gate report

Before any write push, report:

1. **Lint**: pass, or specific rule hits
2. **Validate**: pass, or broken links / integrity issues
3. **Diff**: which pages changed, scope summary
4. **Risk**: any delete, force, template-scope, or category-scope concerns
5. **Next**: `wikitool push --dry-run --summary "..."`

Do not approve `--force` without explicit user instruction.
