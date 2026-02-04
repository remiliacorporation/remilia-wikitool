# Wikitool How-To

Task-focused recipes for common workflows.

## Directory layouts (auto-detected)

**Standalone mode**:
- `<project>/wikitool/` - this tool
- `<project>/wiki_content/` - articles
- `<project>/templates/` - templates/modules
- `<project>/.env` - bot credentials

**Embedded mode**:
- `<wiki>/custom/wikitool/` - this tool
- `<wiki>/wiki_content/` - articles
- `<wiki>/custom/templates/` - templates/modules
- `<wiki>/.env` - bot credentials

Run commands from `<wikitool-dir>` (the tool auto-detects the layout).

## First-time setup

```bash
cd <wikitool-dir>
bun install && bun run build
bun run wikitool init
bun run wikitool pull --full --all   # Pull everything
```

Or use the bootstrap script from the project root (parent of `<wikitool-dir>`, pulls content by default):
```bash
scripts/bootstrap-windows.ps1   # Windows
scripts/bootstrap-macos.sh      # macOS/Linux
```

Skip the pull if you only want tools installed:
```bash
scripts/bootstrap-windows.ps1 -NoPull   # Windows
scripts/bootstrap-macos.sh --no-pull    # macOS/Linux
```

## Understanding namespaces

By default, commands only operate on Main namespace (articles). Use flags for other content:

| Content | Flag | Path |
|---------|------|------|
| Articles | *(default)* | `wiki_content/Main/` |
| Categories | `--categories` | `wiki_content/Category/` |
| Templates | `--templates` | `templates/` (standalone) or `custom/templates/` (embedded) |
| Everything | `--all` | All above |

## Pull latest content

```bash
cd <wikitool-dir>
bun run wikitool pull              # Incremental (Main namespace only)
bun run wikitool pull --full       # Full refresh (Main namespace only)
bun run wikitool pull --full --all # Full refresh (everything)
```

## Pull by namespace

```bash
cd <wikitool-dir>
bun run wikitool pull --templates    # Template + Module namespaces
bun run wikitool pull --categories   # Category namespace
```

## Review local changes

```bash
cd <wikitool-dir>
bun run wikitool diff
bun run wikitool status --modified
```

## Push changes safely

```bash
cd <wikitool-dir>
bun run wikitool push --dry-run -s "Edit summary"
bun run wikitool push -s "Edit summary"
```

## Full refresh + validation

```bash
# From project root
scripts/wikitool-full-refresh.ps1   # Windows
scripts/wikitool-full-refresh.sh    # macOS / Linux
```

## Validate content and export a report

```bash
cd <wikitool-dir>
bun run wikitool validate --report wikitool_exports/validation-report.md --format md --include-remote
```

## Lint Lua modules

```bash
cd <wikitool-dir>
bun run wikitool lint
```

## Fetch or export external wiki pages

```bash
cd <wikitool-dir>
bun run wikitool fetch "https://en.wikipedia.org/wiki/Example"
bun run wikitool export "https://en.wikipedia.org/wiki/Example" -o exports/example.md
```

## Import Cargo data (CSV/JSON)

```bash
cd <wikitool-dir>
bun run wikitool import cargo data.csv --table=Projects --write
```

## Rebuild or query the index

```bash
cd <wikitool-dir>
bun run wikitool index rebuild
bun run wikitool index backlinks "Main Page"
```

## Inspect SEO / Network / Performance

```bash
cd <wikitool-dir>
bun run wikitool seo inspect "Main Page"
bun run wikitool net inspect "Main Page" --limit 25
bun run wikitool perf lighthouse "Main Page"
```

## Set up credentials for push

Create `.env` in the **project root** (parent of `<wikitool-dir>`):

```bash
WIKI_BOT_USER=YourUsername@BotName
WIKI_BOT_PASS=your-bot-password
```

Get credentials from Special:BotPasswords on the wiki.

## Troubleshooting

### "Bun not found"
Run the OS bootstrap script in the repo root:

```bash
scripts/bootstrap-windows.ps1
scripts/bootstrap-macos.sh
scripts/bootstrap-linux.sh
```

### "Login failed"
Confirm `.env` contains:

```
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

### "Wiki has newer changes"
Pull and re-apply your edits:

```bash
cd <wikitool-dir>
bun run wikitool pull
```

### Push shows "new" pages after fresh pull

If `push --dry-run` shows pages as "new" right after pulling, these are likely Category pages that weren't included in the default pull:

```bash
cd <wikitool-dir>
bun run wikitool pull --categories   # Sync category pages
```

Or pull everything: `bun run wikitool pull --full --all`
