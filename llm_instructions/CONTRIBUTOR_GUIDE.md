# Remilia Wiki - Claude Contributor Guide

Welcome to the Remilia Wiki contributor documentation. This guide covers everything you need to write articles using Claude and the wikitool CLI.

These instructions are optimized for Claude (Anthropic) but can work with other LLMs.

## Quick Start

### For Claude Projects (claude.ai/projects)

1. Create a new Claude Project
2. Upload all files from `llm_instructions/` to the project knowledge
3. Set `claude_project_instructions.txt` as the project instructions
4. Start chatting - Claude will follow the wiki writing guidelines

### For Claude Code / CLI

1. Claude Code reads `CLAUDE.md` and these instruction files automatically
2. Follow `SETUP.md` for setup
3. Use wikitool to pull articles, edit locally, then push

### Key Reference Files

1. `ai_agent_instructions.txt` - Main writing guidelines
2. `template_reference.txt` - Available templates
3. `category_reference.txt` - Standard categories
4. `ai_writing_pitfalls.txt` - Common mistakes to avoid

### For Bulk Editing with wikitool (preferred)

1. Follow the setup guide: `SETUP.md`
2. Create a bot password at `Special:BotPasswords` (admins only) with grants:
   - Basic rights
   - High-volume access
   - Edit pages
3. Copy `.env.template` to the project root `.env` (parent of `<wikitool-dir>`) and fill in:
   - `WIKI_BOT_USER=Username@BotName`
   - `WIKI_BOT_PASS=your-bot-password`
4. Use wikitool:
   - `bun run wikitool pull`
   - `bun run wikitool diff`
   - `bun run wikitool push --dry-run -s "Summary"`
   - `bun run wikitool push -s "Summary"`
5. See `docs/wikitool/reference.md` for the full command reference

## File Overview

### Instruction Files (llm_instructions/)

| File | Purpose |
|------|---------|
| `ai_agent_instructions.txt` | **Main writing guidelines** - read this first |
| `claude_project_instructions.txt` | Short summary for Claude Projects |
| `template_reference.txt` | All available templates and their usage |
| `category_reference.txt` | Standard categories with examples |
| `category_list.txt` | Quick category reference |
| `ai_writing_pitfalls.txt` | Common AI mistakes to avoid |
| `article_template.txt` | Article structure template |
| `remiliawiki_extension_instructions.txt` | MediaWiki extensions guide |
| `wikipedia_manual_of_style.txt` | Wikipedia MoS reference |

### Tools (layout overview)

Standalone mode uses `<project>/wikitool/` with sibling `templates/`. Embedded mode uses `<wiki>/custom/wikitool/` with `custom/templates/`.

| Directory | Purpose |
|-----------|---------|
| `wikitool/` | CLI for sync, validation, search, imports |
| `templates/` | Template and module source files |
| `mediawiki/` | Site-wide CSS/JS |
| `d3charts/` | D3.js charting system |
| `darkmode/` | Dark mode gadget |

## Article Essentials

Every article MUST have:

```wikitext
{{SHORTDESC:Brief one-line description}}
{{Article quality|unverified}}

'''Article Title''' is the opening sentence defining the subject.

== Section heading ==
Content here...

== References ==
{{Reflist}}

[[Category:Remilia]]
[[Category:Appropriate Category]]
```

### Key Rules

1. **Output format**: Raw MediaWiki wikitext (never Markdown)
2. **Quotes**: Use straight quotes (" and '), never curly
3. **Headings**: Sentence case (`== Early life ==`, not `== Early Life ==`)
4. **Citations**: 2-5 per short article, focus on specific claims
5. **Categories**: 2-4 from `category_reference.txt`

### Citation Guidelines

**DO cite:**
- Specific dates, names, events
- Direct quotations
- Statistics and data points
- Controversial claims
- Reception/impact claims

**DON'T cite:**
- Common knowledge (e.g., "NFTs are digital tokens")
- General background context
- Technical explanations

**Leave blank:** All archive fields (`archive-url`, `archive-is`, `screenshot`)

## Using wikitool (preferred)

### Setup

```bash
# From repo root
scripts/bootstrap-windows.ps1   # Windows
scripts/bootstrap-macos.sh      # macOS
scripts/bootstrap-linux.sh      # Linux
```

Copy `.env.template` to the project root `.env` and set bot credentials:

```
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

### Common Commands

```bash
cd <wikitool-dir>

# Pull all articles
bun run wikitool pull

# Check local changes
bun run wikitool diff

# Push changes (safe workflow)
bun run wikitool push --dry-run -s "Edit summary"
bun run wikitool push -s "Edit summary"

# Templates
bun run wikitool pull --templates
bun run wikitool push --templates --dry-run -s "Template update"
bun run wikitool push --templates -s "Template update"
```

### Reference

See `docs/wikitool/reference.md` for the full command/flag list.

## Claude Writing Tips

### Avoid These Common Mistakes

1. **Over-citation** - Don't cite every sentence
2. **Promotional language** - Avoid "groundbreaking", "revolutionary"
3. **Weasel words** - "Some experts say..." needs attribution
4. **Rule of three** - Don't list three adjectives for emphasis
5. **Elegant variation** - Use consistent terminology, not synonyms
6. **Meta-commentary** - No "In this article, we will..."

### Good Writing Patterns

- Lead with the most important information
- Use specific, verifiable details
- Attribute opinions to specific sources
- Keep prose flowing, minimize lists
- Focus on what the subject IS, not what it represents

## Template Quick Reference

### Essential Templates

```wikitext
{{SHORTDESC:Brief description}}
{{Article quality|unverified}}
{{Reflist}}
```

### Infoboxes

```wikitext
{{Infobox person|name=...|image=...|occupation=...}}
{{Infobox organization|name=...|image=...|founded=...}}
{{Infobox NFT collection|name=...|image=...|supply=...}}
```

### Citations

```wikitext
{{Cite web|url=...|title=...|date=...|website=...}}
{{Cite tweet|user=...|date=...|tweet=...|url=...}}
{{Cite news|url=...|title=...|date=...|work=...}}
```

### Navigation

```wikitext
{{Main|Article Name}}
{{See also|Article 1|Article 2}}
{{Further|Article Name}}
```

## Category Quick Reference

**Always include:** `[[Category:Remilia]]` for Remilia-related content

**Content types:**
- `[[Category:People]]` - Biographical articles
- `[[Category:Organizations]]` - Groups and companies
- `[[Category:NFT Collections]]` - NFT projects (non-PFP)
- `[[Category:PFP Projects]]` - Profile picture collections
- `[[Category:Philosophical Concepts]]` - Theory and ideas
- `[[Category:Internet Culture]]` - Online phenomena

**Contextual:**
- `[[Category:New Net Art]]` - Core NNA concepts
- `[[Category:NYC Downtown Art Scene]]` - NYC/Dimes Square context

## Getting Help

- Check `ai_agent_instructions.txt` for detailed guidance
- Review existing verified articles for examples
- Ask in the wiki's discussion channels

## Contributing to This Documentation

These instruction files are maintained in the wiki repository under `llm_instructions/`. To suggest improvements:

1. Edit files locally
2. Test with AI tools
3. Submit a pull request

Keep documentation:
- Concise and scannable
- Focused on practical guidance
- Consistent across files
