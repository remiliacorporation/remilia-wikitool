# CLAUDE.md

Guidance for Claude Code when working with remilia-wikitool.

## Project Overview

**remilia-wikitool** is a CLI for managing RemiliaWiki content locally.
Pull articles, edit them with AI assistance, and push changes back.

**Target Wiki:** https://wiki.remilia.org
**Runtime:** Bun 1.1+

## Quick Start

```bash
mkdir remilia-project
cd remilia-project
git clone https://github.com/remilia-collective/remilia-wikitool.git wikitool
cd wikitool
bun install && bun run build
bun run wikitool init
```

Copy credentials to the project root:

```bash
# From inside wikitool/
cp .env.template ../.env
```

Then pull content:

```bash
bun run wikitool pull --full --all
```

## Repository Structure

```
project/
+-- wikitool/              # This repo
    +-- packages/core/     # API, sync, storage, parser
    +-- packages/cli/      # CLI commands
    +-- llm_instructions/  # Writing guides
    +-- config/            # Parser configuration
+-- wiki_content/          # Article files (sibling)
    +-- Main/              # Main namespace articles
    +-- Category/          # Category pages
+-- templates/             # Template files (sibling)
    +-- cite/              # Citation templates
    +-- infobox/           # Infobox templates
    +-- navbox/            # Navigation templates
```

## Writing Guidelines

See `llm_instructions/` for article conventions:

| File | Purpose |
|------|---------|
| `writing_guide.md` | Core writing guide — workflow, sourcing, citations |
| `style_rules.md` | Natural writing antipatterns — read before every article |
| `article_structure.md` | Required article skeleton and section patterns |
| `extensions.md` | Content extension tags (math, code, video, tabs) |

Template parameters and categories are looked up live:

```bash
bun run wikitool context --template "Infobox person"   # Template params from DB
bun run wikitool search "Category:"                     # Valid categories from DB
bun run wikitool docs search "extension name"           # Imported extension docs
```

## Available Skills

| Skill | Purpose |
|-------|---------|
| `/wikitool <cmd>` | Run wikitool commands |
| `/wt <cmd>` | Shorthand for /wikitool |

Common commands:
- `pull` - Download articles from wiki
- `push` - Upload local changes
- `diff` - Show local modifications
- `status` - Sync status overview
- `search` - Search local content
- `validate` - Check for broken links

## Safety Rules

1. Always run `--dry-run` before push
2. Never use `--force` without explicit user confirmation
3. Check `diff` before pushing changes
4. Review changes with the user before committing

## Standard Workflow

```bash
# 1. Get latest content
bun run wikitool pull

# 2. Edit files in wiki_content/

# 3. Review changes
bun run wikitool diff

# 4. Dry-run push (see what would happen)
bun run wikitool push --dry-run -s "Edit summary"

# 5. Actual push (only after dry-run looks good)
bun run wikitool push -s "Edit summary"
```

## Command Reference

Run `bun run wikitool help` or see `docs/wikitool/reference.md` for full documentation.
