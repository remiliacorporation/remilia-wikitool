# Wikitool Setup Guide

This tutorial gets a fresh clone ready for wikitool. Read-only workflows do not require any MediaWiki secrets. Add bot credentials only if you need to push changes.

## 1) Clone the repo

```bash
git clone <repo-url>
cd wikitool
```

If you already downloaded a zip, unzip it and `cd` into the folder instead.

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

Wikitool auto-detects the layout. For unusual layouts, set:
- `WIKITOOL_PROJECT_ROOT` to the project root
- `WIKITOOL_ROOT` to the wikitool repo root (optional override)

## 2) Run the bootstrap script (installs Bun + tools)

**Quick start (recommended)** - bootstrap and pull content in one step (default):

```bash
# Windows (PowerShell as Admin)
scripts/bootstrap-windows.ps1

# macOS
scripts/bootstrap-macos.sh

# Linux
scripts/bootstrap-linux.sh
```

**Without content pull** - just install tools (you'll pull content manually):

```bash
# Windows
scripts/bootstrap-windows.ps1 -NoPull

# macOS / Linux
scripts/bootstrap-macos.sh --no-pull
scripts/bootstrap-linux.sh --no-pull
```

If you need a clean reinstall, pass the rebuild flag:

```bash
# Windows
scripts/bootstrap-windows.ps1 -Rebuild

# macOS / Linux
scripts/bootstrap-macos.sh --rebuild
scripts/bootstrap-linux.sh --rebuild
```

If you don't need Lua linting (Selene), you can skip it:

```bash
# Windows
scripts/bootstrap-windows.ps1 -SkipSelene

# macOS / Linux
scripts/bootstrap-macos.sh --skip-selene
scripts/bootstrap-linux.sh --skip-selene
```

## 3) Verify the install

```bash
cd <wikitool-dir>
bun run wikitool status
```

You should see page counts unless you used `-NoPull` / `--no-pull`. In that case, the database is empty until you run `wikitool pull`.

## 4) Pull content (if skipped in step 2)

```bash
cd <wikitool-dir>
bun run wikitool pull           # Articles only
bun run wikitool pull --all     # Articles + templates
```

For a full reset and validation run:

```bash
# From repo root
scripts/wikitool-full-refresh.ps1   # Windows
scripts/wikitool-full-refresh.sh    # macOS / Linux
```

## 5) Optional: enable push (requires bot credentials)

Copy the template env file to the **project root** (parent of `<wikitool-dir>`) and fill in bot credentials:

```bash
cp .env.template ../.env
```

Then edit `.env` and set:

```
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

Without these, read-only commands (pull, diff, search, validate, etc.) still work.

### Bot password setup (admins only)

1. Go to `https://wiki.remilia.org/Special:BotPasswords`
2. Create a bot password with grants:
   - Basic rights
   - High-volume access
   - Edit pages
3. Copy the generated `Username@BotName` and password into `.env`

Note: keep `.env` in the project root (parent of `<wikitool-dir>`).

## 6) Command reference

The canonical reference is `docs/wikitool/reference.md` (generated from CLI help).

Quick flag lookup (no file read needed):
```bash
bun run wikitool help <command>
```

Regenerate the reference from the wikitool repo root (or from the parent when embedded):
```bash
scripts/generate-wikitool-reference.ps1   # Windows
scripts/generate-wikitool-reference.sh    # macOS / Linux
```

## 7) If Bun is not installed

Run the bootstrap script for your OS. It installs Bun automatically. If Bun is installed but not in PATH, restart your terminal and re-run the script.

## 8) Agent setup

### Claude Code / Codex CLI

These tools auto-load repo instructions from `CLAUDE.md` and `.claude/rules/`. Just:

1. Run the bootstrap script
2. Start working - Claude Code reads the context automatically

### Claude Desktop (Projects)

Claude Desktop does **not** auto-load repo files. You must manually attach files to your project.

**Minimum files (for wikitool operations):**

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Project overview, structure, rules |
| `SETUP.md` | This setup guide |
| `docs/wikitool/reference.md` | Command reference |

**Full setup (for article writing):**

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Project overview |
| `SETUP.md` | Setup guide |
| `docs/wikitool/reference.md` | Command reference |
| `llm_instructions/ai_agent_instructions.txt` | Main writing guidelines |
| `llm_instructions/template_reference.txt` | All templates (large) |
| `llm_instructions/category_reference.txt` | Category system |
| `llm_instructions/ai_writing_pitfalls.txt` | Common mistakes |
| `llm_instructions/article_template.txt` | Article structure |

**How to attach in Claude Desktop:**

1. Create a new Project at claude.ai/projects
2. Click "Add to project knowledge"
3. Upload the files listed above
4. Optionally set `llm_instructions/claude_project_instructions.txt` as project instructions

**What you lose vs Claude Code:**

- No `/skill` commands (these are Claude Code specific)
- No automatic rule loading from `.claude/rules/`
- Must manually copy `bun run wikitool` commands from reference.md

## 9) Maintenance / updates

```bash
# Reinstall dependencies + tools
scripts/setup-tools.ps1   # Windows
scripts/setup-tools.sh    # macOS / Linux

# Refresh Selene only
scripts/setup-selene.ps1  # Windows
scripts/setup-selene.sh   # macOS / Linux
```
