---
name: context
description: Generate AI context bundles with page content, links, categories
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: "<title>" [options]
---

# /wikitool context - AI Context Bundles

Generate structured context bundles for AI consumption, combining page content with metadata, links, and related information.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool context "Milady Maker"              # Summary view
/wikitool context "Milady Maker" --json       # JSON for AI
/wikitool context "Milady Maker" --full       # Include full content
/wikitool context "Milady Maker" --sections 3 # First 3 sections only
/wikitool context "Infobox person" --template --json  # Template context
```

## Output Contents

**Article context includes:**
- Page title, categories, short description
- Section headings
- Outgoing/incoming links
- Templates used
- Related pages

**Template context includes:**
- Template/module source
- Parameters and usage
- Pages using this template
- Related templates

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool context $ARGUMENTS
```
