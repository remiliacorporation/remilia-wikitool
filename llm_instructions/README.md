# LLM Instructions for Remilia Wiki

Writing guidelines for AI-assisted article creation on [wiki.remilia.org](https://wiki.remilia.org).

These instructions are designed for Claude (Anthropic) but work with any capable LLM. They produce encyclopedic, Wikipedia-style articles in MediaWiki wikitext.

## Files

| File | Purpose |
|------|---------|
| `writing_guide.md` | **Start here.** Core writing workflow, sourcing, citations, content rules. |
| `style_rules.md` | Natural writing rules — phrases to avoid, formatting, citation hygiene. Read before every article. |
| `article_structure.md` | Structural template for articles (required skeleton, section patterns). |
| `extensions.md` | Quick reference for content extension tags (math, code, video, tabs). |

## Usage

### With wikitool (recommended)

These instructions pair with [remilia-wikitool](https://github.com/remiliacorporation/remilia-wikitool) for a complete article workflow:

```bash
bun run wikitool pull                              # Get latest articles
# ... write/edit articles following these guides ...
bun run wikitool diff                              # Review changes
bun run wikitool push --dry-run -s "Summary"       # Preview push
bun run wikitool push -s "Summary"                 # Push to wiki
```

Template parameters and categories are looked up live from the database:

```bash
bun run wikitool context --template "Infobox person"   # Template params
bun run wikitool docs search "embed video"             # Extension docs
```

### With Claude Projects (claude.ai)

1. Create a new Claude Project
2. Upload all `.md` files from this directory to project knowledge
3. Start chatting — Claude will follow the writing guidelines

### With Claude Code

If using wikitool as a submodule, Claude Code reads these files via skill directives in `.claude/skills/`.

## Style rules source

`style_rules.md` is derived from:
- [Wikipedia:Signs of AI writing](https://en.wikipedia.org/wiki/Wikipedia:Signs_of_AI_writing)
- [Wikipedia:Manual of Style](https://en.wikipedia.org/wiki/Wikipedia:Manual_of_Style)

Both sources can be refreshed via `bun run wikitool export <url>`.
