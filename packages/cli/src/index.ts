#!/usr/bin/env bun
/**
 * @wikitool/cli - Command-line interface for wikitool
 *
 * Provides commands for syncing wiki content between local files and MediaWiki.
 */

import { resolve } from 'node:path';
import { existsSync } from 'node:fs';
import { config as dotenvConfig } from 'dotenv';
import { Command } from 'commander';
import chalk from 'chalk';
import { VERSION, loadNamespaceConfig } from '@wikitool/core';
import { detectProjectContext } from './utils/context.js';

function resolveContextOrExit() {
  try {
    return detectProjectContext();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(chalk.red(message));
    process.exit(1);
  }
}

// Load .env from project root first (where user credentials live)
const ctxPre = resolveContextOrExit();
const projectEnv = resolve(ctxPre.projectRoot, '.env');
const wikitoolEnv = resolve(ctxPre.wikitoolRoot, '.env');

if (existsSync(projectEnv)) {
  dotenvConfig({ path: projectEnv });
} else if (existsSync(wikitoolEnv)) {
  dotenvConfig({ path: wikitoolEnv });
}

// Re-evaluate context after loading .env (for WIKITOOL_PROJECT_ROOT / WIKITOOL_ROOT)
const ctx = resolveContextOrExit();

// Load namespace configuration at startup
if (existsSync(ctx.configPath)) {
  loadNamespaceConfig(ctx.configPath);
} else {
  loadNamespaceConfig();
}

// Import commands
import { initCommand } from './commands/init.js';
import { pullCommand } from './commands/pull.js';
import { pushCommand } from './commands/push.js';
import { diffCommand } from './commands/diff.js';
import { statusCommand } from './commands/status.js';
import { fetchCommand } from './commands/fetch.js';
import { exportCommand } from './commands/export.js';
import { searchCommand } from './commands/search.js';
import { searchExternalCommand } from './commands/search-external.js';
import { dbCommand } from './commands/db.js';
import {
  docsImportCommand,
  docsImportTechnicalCommand,
  docsListCommand,
  docsUpdateCommand,
  docsRemoveCommand,
  docsSearchCommand,
} from './commands/docs.js';
import {
  lspGenerateConfigCommand,
  lspStatusCommand,
  lspInfoCommand,
} from './commands/lsp.js';
import { indexCommand } from './commands/index-cmd.js';
import { validateCommand } from './commands/validate.js';
import { deleteCommand } from './commands/delete.js';
import { contextCommand } from './commands/context.js';
import { lintCommand } from './commands/lint.js';
import { importCargoCommand } from './commands/import.js';
import { seoInspectCommand } from './commands/seo.js';
import { netInspectCommand } from './commands/net.js';
import { perfLighthouseCommand } from './commands/perf.js';

const program = new Command();

program
  .name('wikitool')
  .description('Unified MediaWiki tooling for Remilia Wiki')
  .version(VERSION);

// Initialize
program
  .command('init')
  .description('Initialize wikitool (create database, setup config)')
  .option('--from-files', 'Initialize database from existing local files')
  .option('--templates', 'Include templates when initializing from files')
  .action(initCommand);

// Pull from wiki
program
  .command('pull')
  .description('Pull pages from wiki')
  .option('--full', 'Full refresh (ignore last pull timestamp)')
  .option('--overwrite-local', 'Overwrite locally modified files during pull')
  .option('-c, --category <name>', 'Filter by category')
  .option('--templates', 'Pull templates instead of articles')
  .option('--categories', 'Pull Category: namespace pages')
  .option('--all', 'Pull everything (articles, categories, and templates)')
  .action(pullCommand);

// Context bundle
program
  .command('context')
  .description('Show context bundle for a page')
  .argument('<title>', 'Page title')
  .option('--json', 'Output JSON for AI consumption')
  .option('--no-meta', 'Omit meta block from JSON output')
  .option('--sections <n>', 'Limit number of sections returned')
  .option('--full', 'Include full page content')
  .option('--template', 'Treat the title as a template name')
  .action(contextCommand);

// Push to wiki
program
  .command('push')
  .description('Push local changes to wiki')
  .requiredOption('-s, --summary <text>', 'Edit summary (required)')
  .option('--dry-run', 'Preview changes without pushing')
  .option('--force', 'Push even if wiki has newer changes')
  .option('--delete', 'Delete pages on wiki when local files are removed')
  .option('--templates', 'Push templates instead of articles')
  .option('--categories', 'Push Category: namespace pages only')
  .option('--no-lint', 'Skip Lua linting')
  .option('--lint-strict', 'Treat Lua lint warnings as errors')
  .action(pushCommand);

// Show diff
program
  .command('diff')
  .description('Show local changes')
  .option('--templates', 'Show template changes')
  .option('-v, --verbose', 'Show content diff')
  .action(diffCommand);

// Show status
program
  .command('status')
  .description('Show sync status')
  .option('--modified', 'Only show modified')
  .option('--conflicts', 'Only show conflicts')
  .option('--templates', 'Include templates')
  .action(statusCommand);

// Fetch external wiki content
program
  .command('fetch <url>')
  .description('Fetch content from external wiki')
  .option('--format <type>', 'Output format (wikitext, html)', 'wikitext')
  .option('--save', 'Save to reference library')
  .option('--name <name>', 'Custom name for saved reference')
  .action(fetchCommand);

// Export external wiki to markdown
program
  .command('export <url>')
  .description('Export external wiki page to AI-friendly markdown')
  .option('-o, --output <path>', 'Output path (file or directory for --subpages)')
  .option('--format <format>', 'Output format: markdown (default) or wikitext')
  .option('--code-language <lang>', 'Default language for code blocks (e.g., c, cpp)')
  .option('--no-frontmatter', 'Skip YAML frontmatter')
  .option('--subpages', 'Include all subpages (outputs separate files to directory)')
  .option('--combined', 'With --subpages: combine all pages into single file')
  .action(exportCommand);

// Search local content
program
  .command('search <query>')
  .description('Search local content')
  .option('--tier <tier>', 'Search tier (content, extension, technical)')
  .option('-l, --limit <n>', 'Limit results', '20')
  .action(searchCommand);

// Search external wikis
program
  .command('search-external <query>')
  .description('Search external wikis (Wikipedia, MediaWiki.org, custom domains)')
  .option('--wiki <wiki>', 'Wiki to search (wikipedia, mediawiki, commons, wikidata)', 'wikipedia')
  .option('--lang <lang>', 'Language code for Wikipedia', 'en')
  .option('--domain <domain>', 'Custom MediaWiki domain (e.g., "meta.miraheze.org")')
  .option('--api-url <url>', 'Override API URL for custom domains')
  .option('-l, --limit <n>', 'Limit results', '10')
  .action(searchExternalCommand);

// Database operations
const db = program
  .command('db')
  .description('Database operations')
  .action(() => db.help());

db.command('stats')
  .description('Show database statistics')
  .action(() => dbCommand('stats'));

db.command('sync')
  .description('Sync files with database (repair)')
  .action(() => dbCommand('sync'));

db.command('migrate')
  .description('Run pending migrations')
  .option('--status', 'Show migration status instead of running migrations')
  .option('--validate', 'Validate schema without running migrations')
  .action((options) => dbCommand('migrate', options));

// Documentation operations
const docs = program
  .command('docs')
  .description('MediaWiki documentation management')
  .action(() => docs.help());

docs
  .command('import [extensions...]')
  .description('Import extension documentation from mediawiki.org')
  .option('--no-subpages', 'Skip subpages')
  .option('--installed', 'Import all extensions from LocalSettings.php')
  .action(docsImportCommand);

docs
  .command('import-technical [pages...]')
  .description('Import technical documentation from mediawiki.org')
  .option('--subpages', 'Include subpages')
  .option('--hooks', 'Import all hook documentation')
  .option('--config', 'Import configuration variable docs')
  .option('--api', 'Import API documentation')
  .option('-l, --limit <n>', 'Limit pages to import', '100')
  .action(docsImportTechnicalCommand);

docs
  .command('list')
  .description('List imported documentation')
  .option('--outdated', 'Show only outdated docs')
  .option('--type <type>', 'Filter by technical doc type')
  .action(docsListCommand);

docs
  .command('update')
  .description('Update outdated documentation')
  .action(docsUpdateCommand);

docs
  .command('remove <target>')
  .description('Remove documentation (extension name or doc type)')
  .action(docsRemoveCommand);

docs
  .command('search <query>')
  .description('Search documentation')
  .option('--tier <tier>', 'Search tier (extension, technical)')
  .option('-l, --limit <n>', 'Limit results', '20')
  .action(docsSearchCommand);

// LSP operations
program
  .command('lsp:generate-config')
  .description('Generate VS Code configuration for wikitext LSP')
  .action(lspGenerateConfigCommand);

program
  .command('lsp:status')
  .description('Show LSP configuration status')
  .action(lspStatusCommand);

program
  .command('lsp:info')
  .description('Show information about the wikitext LSP')
  .action(lspInfoCommand);

// Validation
program
  .command('validate')
  .description('Validate wiki content for common issues')
  .option('--fix', 'Auto-fix issues (disabled; report-only)')
  .option('--report <file>', 'Export validation report (json or md)')
  .option('--format <format>', 'Report format: json|md', 'json')
  .option('--no-meta', 'Omit meta block from JSON reports')
  .option('--include-remote', 'Include Special: page snapshots in report')
  .option('--remote-limit <n>', 'Limit items per Special: page in report (0 = all)', '200')
  .option('-l, --limit <n>', 'Limit results per category', '10')
  .action(validateCommand);

// Lint modules
program
  .command('lint [title]')
  .description('Lint Lua modules with Selene')
  .option('--format <format>', 'Output format: text|json', 'text')
  .option('--strict', 'Treat warnings as errors')
  .option('--no-meta', 'Omit meta block from JSON output')
  .action(lintCommand);

// SEO inspection
const seo = program
  .command('seo')
  .description('SEO inspection tools')
  .action(() => seo.help());

seo
  .command('inspect <target>')
  .description('Inspect SEO meta tags for a page or URL')
  .option('--json', 'Output JSON for AI consumption')
  .option('--no-meta', 'Omit meta block from JSON output')
  .option('--url <url>', 'Override target URL')
  .action((target, options) => seoInspectCommand(target, options));

// Network inspection
const net = program
  .command('net')
  .description('Network inspection tools')
  .action(() => net.help());

net
  .command('inspect <target>')
  .description('Inspect page resources and cache headers')
  .option('--limit <n>', 'Limit number of resources to probe', '25')
  .option('--no-probe', 'Skip HEAD probes (faster, no size/cache info)')
  .option('--json', 'Output JSON for AI consumption')
  .option('--no-meta', 'Omit meta block from JSON output')
  .option('--url <url>', 'Override target URL')
  .action((target, options) => netInspectCommand(target, options));

// Performance diagnostics
const perf = program
  .command('perf')
  .description('Performance diagnostics')
  .action(() => perf.help());

perf
  .command('lighthouse [target]')
  .description('Run a Lighthouse audit for a page or URL')
  .option('--output <format>', 'Output format: html|json', 'html')
  .option('--out <path>', 'Report output path')
  .option('--categories <list>', 'Comma-separated categories (performance,seo,...)')
  .option('--chrome-flags <flags>', 'Pass Chrome flags to Lighthouse')
  .option('--show-version', 'Print resolved Lighthouse binary + version and exit')
  .option('--json', 'Output JSON summary')
  .option('--no-meta', 'Omit meta block from JSON output')
  .option('--url <url>', 'Override target URL')
  .action((target, options) => perfLighthouseCommand(target, options));

// Import helpers
const importCmd = program
  .command('import')
  .description('Import data into local wiki files')
  .action(() => importCmd.help());

importCmd
  .command('cargo <path>')
  .description('Import CSV/JSON into Cargo pages')
  .requiredOption('--table <name>', 'Cargo table name')
  .option('--type <type>', 'Input type: csv|json')
  .option('--template <name>', 'Template wrapper name')
  .option('--title-field <field>', 'Field name to use as the page title')
  .option('--title-prefix <prefix>', 'Prefix for generated page titles')
  .option('--category <name>', 'Category to add to generated pages')
  .option('--mode <mode>', 'create|update|upsert', 'create')
  .option('--write', 'Write files (default: dry-run)')
  .option('--format <format>', 'Output format: text|json', 'text')
  .option('--article-header', 'Add SHORTDESC + Article quality header for main namespace')
  .option('--no-meta', 'Omit meta block from JSON output')
  .action((path, options) => importCargoCommand(path, options));

// Delete page
program
  .command('delete <title>')
  .description('Delete a page from the wiki (requires authentication)')
  .requiredOption('--reason <text>', 'Reason for deletion (required)')
  .option('--no-backup', 'Skip backup (not recommended)')
  .option('--backup-dir <path>', 'Custom backup directory')
  .option('--dry-run', 'Preview deletion without making changes')
  .action(deleteCommand);

// Index operations (link graph)
const index = program
  .command('index')
  .description('Link index operations (rebuild, stats, queries)')
  .action(() => index.help());

index
  .command('rebuild')
  .description('Rebuild link index from page content')
  .action(() => indexCommand('rebuild'));

index
  .command('stats')
  .description('Show index statistics')
  .option('-v, --verbose', 'Show detailed breakdowns')
  .option('--json', 'Output JSON stats')
  .option('--no-meta', 'Omit meta block from JSON output')
  .action((options) => indexCommand('stats', undefined, options));

index
  .command('backlinks <title>')
  .description('Show pages that link to a title')
  .option('-l, --limit <n>', 'Limit results', '50')
  .action((title, options) => indexCommand('backlinks', title, options));

index
  .command('orphans')
  .description('Show pages with no incoming links')
  .option('-l, --limit <n>', 'Limit results', '50')
  .action((options) => indexCommand('orphans', undefined, options));

index
  .command('prune-categories')
  .description('List or delete empty categories')
  .option('--apply', 'Delete empty categories')
  .option('--min-members <n>', 'Treat categories with fewer than N members as empty', '1')
  .option('-l, --limit <n>', 'Limit results', '50')
  .action((options) => indexCommand('prune-categories', undefined, options));

// Parse and run
program.parse();
