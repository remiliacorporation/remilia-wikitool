/**
 * lsp command - LSP configuration and setup
 *
 * Provides commands for configuring the wikitext LSP server for RemiliaWiki.
 */

import chalk from 'chalk';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { existsSync, readFileSync } from 'node:fs';
import { printSuccess, printInfo, printWarning } from '../utils/format.js';

// Get the wikitool root directory
function getWikitoolRoot(): string {
  // This file is at packages/cli/src/commands/lsp.ts
  // Wikitool root is at ../../..
  const currentDir = dirname(fileURLToPath(import.meta.url));
  return resolve(currentDir, '..', '..', '..', '..');
}

// Get the config path
function getConfigPath(): string {
  const wikitoolRoot = getWikitoolRoot();
  return resolve(wikitoolRoot, 'config', 'remilia-parser.json');
}

/**
 * Generate VS Code settings for wikiparser extension
 */
export async function lspGenerateConfigCommand(): Promise<void> {
  const configPath = getConfigPath();

  if (!existsSync(configPath)) {
    console.error(chalk.red('Error:'), 'Parser config not found at', configPath);
    console.log('Run from the wikitool directory or ensure config/remilia-parser.json exists.');
    process.exit(1);
  }

  console.log(chalk.bold('\nVS Code Configuration for RemiliaWiki\n'));
  console.log('Add the following to your VS Code settings.json:\n');

  const settings = {
    'wikiparser.articlePath': 'https://wiki.remilia.org/wiki/$1',
    'wikiparser.config': configPath.replace(/\\/g, '/'),
    'wikiparser.linter.enable': true,
    'wikiparser.linter.severity': 'errors and warnings',
    'wikiparser.inlay': true,
    'wikiparser.completion': true,
    'wikiparser.color': true,
    'wikiparser.hover': true,
    'wikiparser.signature': true,
  };

  console.log(chalk.cyan(JSON.stringify(settings, null, 2)));

  console.log(chalk.bold('\n\nSetup Instructions:\n'));
  console.log('1. Install the "Wikitext" VS Code extension by Bhsd');
  console.log('2. Open VS Code settings (Cmd/Ctrl + ,)');
  console.log('3. Search for "wikiparser"');
  console.log('4. Configure the settings as shown above');
  console.log('\nAlternatively, add the JSON above to your settings.json file directly.');

  console.log(chalk.bold('\n\nClaude Code Integration:\n'));
  console.log('The MCP server provides wiki tools directly to Claude Code.');
  console.log('No additional LSP configuration is needed for Claude Code.');
  console.log('Claude can use wiki_search, wiki_pull, wiki_push, etc. tools directly.\n');

  printSuccess('Configuration generated successfully');
}

/**
 * Show LSP status and information
 */
export async function lspStatusCommand(): Promise<void> {
  const configPath = getConfigPath();
  const configExists = existsSync(configPath);

  console.log(chalk.bold('\nLSP Status\n'));

  console.log('Config file:', configExists ? chalk.green('✓ Found') : chalk.red('✗ Missing'));
  console.log('  Path:', configPath);

  if (configExists) {
    try {
      const content = readFileSync(configPath, 'utf-8');
      const config = JSON.parse(content);
      const extCount = config.ext?.length || 0;
      const nsCount = Object.keys(config.namespaces || {}).length;

      console.log('\nConfiguration:');
      console.log(`  Extension tags: ${extCount}`);
      console.log(`  Namespaces: ${nsCount}`);
      console.log(`  Interwiki prefixes: ${config.interwiki?.length || 0}`);
    } catch (error) {
      printWarning('Could not parse config file');
    }
  }

  // Check if wikitext-lsp is available
  try {
    const { resolve } = await import('node:path');
    const wikitextLspPath = resolve(getWikitoolRoot(), 'node_modules', 'wikitext-lsp');
    const lspExists = existsSync(wikitextLspPath);

    console.log('\nwikitext-lsp:', lspExists ? chalk.green('✓ Installed') : chalk.red('✗ Not found'));

    if (lspExists) {
      const pkgPath = resolve(wikitextLspPath, 'package.json');
      if (existsSync(pkgPath)) {
        const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'));
        console.log(`  Version: ${pkg.version}`);
      }
    }
  } catch {
    printWarning('Could not check wikitext-lsp installation');
  }

  console.log('\nServer command:', chalk.cyan('bunx wikitext-lsp'));
  console.log();
}

/**
 * Show information about VS Code extension
 */
export async function lspInfoCommand(): Promise<void> {
  console.log(chalk.bold('\nWikitext LSP Information\n'));

  console.log(chalk.cyan('VS Code Extension'));
  console.log('  Name: Wikitext');
  console.log('  Publisher: Bhsd');
  console.log('  ID: bhsd.vscode-extension-wikiparser');
  console.log('  Install: ext install bhsd.vscode-extension-wikiparser');
  console.log('  Marketplace: https://marketplace.visualstudio.com/items?itemName=Bhsd.vscode-extension-wikiparser');

  console.log(chalk.cyan('\nFeatures'));
  console.log('  • Syntax highlighting for .wiki files');
  console.log('  • Error detection and quick fixes');
  console.log('  • Template parameter completion');
  console.log('  • Hover information');
  console.log('  • Document symbols (headings, templates)');
  console.log('  • Link navigation');
  console.log('  • Color preview for CSS');
  console.log('  • Inlay hints for template parameters');

  console.log(chalk.cyan('\nSupported File Types'));
  console.log('  • .wiki');
  console.log('  • .wikitext');
  console.log('  • .mediawiki');

  console.log(chalk.cyan('\nRemiliaWiki Configuration'));
  console.log('  Run: bun run wikitool lsp:generate-config');
  console.log('  This generates VS Code settings for wiki.remilia.org');
  console.log();
}

