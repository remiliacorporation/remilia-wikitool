# Wikitool

Unified MediaWiki tooling for Remilia Wiki. TypeScript monorepo providing CLI and core library.

**Runtime**: Bun 1.1+

## Quick Start

```bash
cd <wikitool-dir>
bun install
bun run build

# Initialize database
bun run wikitool init

# Pull content from wiki
bun run wikitool pull

# Edit files in wiki_content/

# Review and push changes
bun run wikitool diff
bun run wikitool push --dry-run -s "Edit summary"
bun run wikitool push -s "Edit summary"
```

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

Run commands from `<wikitool-dir>`; the tool auto-detects the layout.

## Namespaces

By default, commands operate on the **Main** namespace only. Use flags to include other namespaces:

| Content Type | Pull Flag | Push Flag | Local Path |
|--------------|-----------|-----------|------------|
| Articles | *(default)* | *(default)* | `wiki_content/Main/` |
| Categories | `--categories` | `--categories` | `wiki_content/Category/` |
| Templates/Modules | `--templates` | `--templates` | `templates/` (standalone) or `custom/templates/` (embedded) |
| Everything | `--all` | N/A | All paths |

**First-time setup** - pull everything:
```bash
bun run wikitool pull --full --all
```

## Editor Bootstrap (recommended)

Run one script from the repo root to install prerequisites and set up wikitool:

- Windows (PowerShell, admin): `scripts/bootstrap-windows.ps1`
- macOS (bash/zsh): `scripts/bootstrap-macos.sh`
- Linux (bash): `scripts/bootstrap-linux.sh`

These scripts install Bun, set up wikitool, install a git hook to strip co-author lines, and **pull wiki content by default**.
They do **not** install Git; Git is only needed to clone or update the repo.
Use `-NoPull` / `--no-pull` to skip the content pull, or `--rebuild` (or `--fix`) to force a clean dependency/tool reinstall:

- Windows (skip pull): `scripts/bootstrap-windows.ps1 -NoPull`
- macOS/Linux (skip pull): `scripts/bootstrap-macos.sh --no-pull`
- Windows: `scripts/bootstrap-windows.ps1 -Rebuild`
- macOS/Linux: `scripts/bootstrap-macos.sh --rebuild`
 - Windows (skip Selene): `scripts/bootstrap-windows.ps1 -SkipSelene`
 - macOS/Linux (skip Selene): `scripts/bootstrap-macos.sh --skip-selene`

## Full Refresh + Validation

From the repo root, run one of:

- Windows: `scripts/wikitool-full-refresh.ps1`
- macOS / Linux: `scripts/wikitool-full-refresh.sh`

This resets the local wikitool DB, pulls all articles + templates, validates content (exporting a report to `wikitool_exports/validation-report.md`), and runs the wikitool test suite (`bun test tests`). Indexes are updated incrementally during pull; use `wikitool index rebuild` only if you suspect drift.

## Documentation

- `SETUP.md` - editor/agent setup walkthrough
- `docs/wikitool/how-to.md` - task recipes
- `docs/wikitool/reference.md` - canonical command/flag reference
- `docs/wikitool/explanation.md` - architecture and rationale

## Commands

For the canonical command/flag reference, see `docs/wikitool/reference.md`.
Regenerate it from `<wikitool-dir>`:

```bash
# Windows
scripts/generate-wikitool-reference.ps1

# macOS / Linux
scripts/generate-wikitool-reference.sh
```

CLI help is always available:

```bash
cd <wikitool-dir>
bun run wikitool help
bun run wikitool help <command>
```

## Packages

| Package | Description |
|---------|-------------|
| `@wikitool/core` | Core functionality - API client, sync, storage, parsing, indexing |
| `@wikitool/cli` | Command-line interface |

## Architecture

```
wikitool/
├── packages/
│   ├── core/           # @wikitool/core
│   │   ├── api/        # MediaWiki API client
│   │   ├── sync/       # Sync operations
│   │   ├── storage/    # SQLite database with migrations
│   │   ├── storage/    # SQLite database, filesystem I/O
│   │   ├── docs/       # Documentation fetcher
│   │   ├── external/   # External wiki client (rate-limited)
│   │   ├── config/     # Namespace configuration loader
│   │   ├── parser/     # Wikitext parser (links, metadata, word count)
│   │   └── index/      # Index rebuild and queries
│   └── cli/            # @wikitool/cli
│       └── commands/   # CLI command handlers
├── config/             # RemiliaWiki parser config
└── data/               # SQLite database (gitignored)
```

## Database

Wikitool uses SQLite for efficient sync state and full-text search:

- **Pages table**: Tracks all synced pages with content hashes, metadata (shortdesc, display_title, word_count)
- **Categories table**: Category relationships
- **Page links table**: Internal/interwiki link graph
- **Template usage table**: Which pages use which templates
- **Redirects table**: Redirect mappings for quick lookup
- **Extension docs table**: Imported extension documentation
- **Technical docs table**: Imported manual/hooks/config docs
- **FTS5 index**: Full-text search across all tiers

## Context Layer (AI)

The index rebuild also populates deterministic context tables to support AI-assisted editing:

- **page_sections**: Lead + headings with section text (plus `page_sections_fts`)
- **template_calls / template_params**: Template invocations with parameters
- **infobox_kv**: Parsed infobox key/value pairs
- **template_metadata**: TemplateData metadata from template pages
- **module_deps**: `require` / `mw.loadData` dependencies for Lua modules

Use `wikitool context "Title" --json` to retrieve a structured context bundle.

## VS Code Integration

1. Install the "Wikitext" VS Code extension by Bhsd
2. Run: `bun run wikitool lsp:generate-config`
3. Copy the settings to your VS Code settings.json

## Environment Variables

Wikitool loads environment variables from `.env` in the **project root** (parent of `<wikitool-dir>`). This file is gitignored.

For unusual layouts, set:
- `WIKITOOL_PROJECT_ROOT` to the project root
- `WIKITOOL_ROOT` to the wikitool repo root (optional override)

**Required for push operations:**
```bash
WIKI_BOT_USER=Username@BotName    # Bot username (from Special:BotPasswords)
WIKI_BOT_PASS=your-bot-password   # Bot password
```

**Optional:**
```bash
WIKI_API_URL=https://wiki.remilia.org/api.php  # API endpoint (default shown)
WIKI_HTTP_TIMEOUT_MS=30000        # Request timeout in ms
WIKI_HTTP_RETRIES=2               # Max retries for read requests
WIKI_HTTP_WRITE_RETRIES=1         # Max retries for write requests
WIKI_HTTP_RETRY_DELAY_MS=500      # Base retry backoff in ms
```

**Note:** Pull operations work without credentials (wiki is public). Push operations require bot credentials.

## Development

```bash
# Install dependencies
bun install

# Build all packages
bun run build

# Run tests
bun run test

# Lint
bun run lint
```

## License

This project is licensed under the MIT License. An additional license text is provided in `LICENSE-VPL`.
