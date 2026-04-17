# Writing Context

Default article-writing context for Remilia Wiki.

This directory is not the global agent instruction surface. It contains editorial and wikitext
guidance used when drafting or revising articles. Agent routing lives in `AGENTS.md` / `CLAUDE.md`;
operator workflows live in `docs/wikitool/guide.md` and the skill wrappers.

For another MediaWiki target, provide host `writing_context/` when building the AI pack with
`--host-project-root`; wikitool packages that host writing profile at the same release-root path.

## Files

| File | Purpose |
|---|---|
| `style_rules.md` | Natural writing rules and AI-writing antipatterns. Read before every article. |
| `article_structure.md` | Required article skeleton and section patterns. |
| `writing_guide.md` | Sourcing, citations, categories, and article workflow. |
| `extensions.md` | Content extension tags and target-local chart contracts. |

## Lookup Boundary

Static writing context is only the baseline. For live target-wiki facts, use wikitool:

```bash
wikitool knowledge article-start "Topic" --intent new --format json
wikitool knowledge contracts search "subject type infobox" --format json
wikitool templates show "Template:Infobox person"
wikitool wiki profile show --format json
wikitool article lint wiki_content/Main/Topic.wiki --format json
```

Do not treat Remilia-specific templates, categories, or D3Charts syntax as portable MediaWiki
features on other target wikis.
