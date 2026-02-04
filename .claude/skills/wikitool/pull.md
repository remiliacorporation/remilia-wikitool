---
name: pull
description: Download articles/templates from the live wiki to local files
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: [options]
---

# /wikitool pull - Download Wiki Content

Pull pages from wiki.remilia.org to local files.

**Default behavior**: Only pulls Main namespace (articles) to `wiki_content/Main/`.

Use `--all` for first-time setup to get everything:
```bash
/wikitool pull --full --all
```

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool pull                    # Pull all articles
/wikitool pull --templates        # Pull all templates
/wikitool pull --full             # Force re-download everything
/wikitool pull --full --overwrite-local  # Overwrite local edits during pull
/wikitool pull --all              # Pull articles + templates
/wikitool pull --category "NFT collections"  # Pull by category
/wikitool pull --categories       # Pull Category: namespace pages
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool pull $ARGUMENTS
```
