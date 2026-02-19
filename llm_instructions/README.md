# LLM Instructions for Remilia Wiki

Writing guidelines for AI-assisted article creation on [wiki.remilia.org](https://wiki.remilia.org).

These instructions are designed for Claude but work with any capable LLM. They target encyclopedic MediaWiki wikitext.

## Files

| File | Purpose |
|------|---------|
| `writing_guide.md` | Start here. Core writing workflow, sourcing, citations, content rules. |
| `style_rules.md` | Natural writing rules and anti-patterns. Read before every article. |
| `article_structure.md` | Required structural template and section patterns. |
| `extensions.md` | Quick reference for content extension tags. |

## Usage

### With wikitool (recommended)

```bash
wikitool pull
# ... write/edit articles following these guides ...
wikitool diff
wikitool push --dry-run --summary "Summary"
wikitool push --summary "Summary"
```

Helpful lookups:

```bash
wikitool context "Template:Infobox person"
wikitool search "Category:"
wikitool docs search "embed video"
```

### With Claude Projects (claude.ai)

1. Create a new Claude Project.
2. Upload all `.md` files from this directory.
3. Start writing with these guidelines as project knowledge.

### With Claude Code

If using wikitool as a submodule, Claude Code reads these files via repo instructions and skills.

## Style rules source

`style_rules.md` is derived from:

- [Wikipedia:Signs of AI writing](https://en.wikipedia.org/wiki/Wikipedia:Signs_of_AI_writing)
- [Wikipedia:Manual of Style](https://en.wikipedia.org/wiki/Wikipedia:Manual_of_Style)

Both sources can be refreshed via `wikitool export <url>`.
