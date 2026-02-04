/**
 * docs command - Manage MediaWiki documentation
 *
 * Import, search, and manage extension and technical documentation from mediawiki.org
 */

import chalk from 'chalk';
import ora from 'ora';
import Table from 'cli-table3';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printWarning, printInfo } from '../utils/format.js';
import {
  fetchExtensionDocs,
  fetchTechnicalDocs,
  getInstalledExtensions,
  formatExpiration,
  isExpired,
  TECHNICAL_DOC_TYPES,
  type TechnicalDocType,
} from '@wikitool/core';
import { resolve } from 'node:path';

// ============================================================================
// Import extension documentation
// ============================================================================

export interface DocsImportOptions {
  subpages?: boolean;
  installed?: boolean;
}

export async function docsImportCommand(
  extensions: string[],
  options: DocsImportOptions
): Promise<void> {
  await withContext(async (ctx) => {
    let extensionsToImport = extensions;

    // If --installed flag, get extensions from LocalSettings.php
    if (options.installed) {
      const localSettingsPath = resolve(ctx.rootDir, 'LocalSettings.php');
      const spinner = ora('Reading LocalSettings.php...').start();

      try {
        extensionsToImport = await getInstalledExtensions(localSettingsPath);
        spinner.succeed(`Found ${extensionsToImport.length} installed extensions`);
        console.log(chalk.dim(extensionsToImport.join(', ')));
      } catch (error) {
        spinner.fail('Failed to read LocalSettings.php');
        const message = error instanceof Error ? error.message : String(error);
        printError(message);
        process.exit(1);
      }
    }

    if (extensionsToImport.length === 0) {
      printError('No extensions specified. Use: docs import Extension:Name or docs import --installed');
      process.exit(1);
    }

    console.log(chalk.bold(`\nImporting documentation for ${extensionsToImport.length} extension(s)...\n`));

    let totalPages = 0;
    let successCount = 0;

    for (const ext of extensionsToImport) {
      // Normalize extension name
      const extName = ext.replace(/^Extension:/, '');
      const spinner = ora(`Importing ${extName}...`).start();

      try {
        const result = await fetchExtensionDocs(extName, {
          includeSubpages: options.subpages !== false,
          onProgress: (current, total, message) => {
            spinner.text = `${extName}: ${message || `${current}/${total}`}`;
          },
        });

        // Store in database
        const docId = ctx.db.upsertExtensionDoc({
          extensionName: result.info.extensionName,
          sourceWiki: result.info.sourceWiki,
          version: result.info.version,
          pagesCount: result.info.pagesCount,
          expiresAt: result.info.expiresAt,
        });

        // Store pages
        for (const page of result.pages) {
          ctx.db.upsertExtensionDocPage({
            extensionId: docId,
            pageTitle: page.pageTitle,
            localPath: page.localPath,
            content: page.content,
            contentHash: page.contentHash,
          });

          // Index for FTS
          ctx.db.indexPage('extension', page.pageTitle, page.content);
        }

        spinner.succeed(`${extName}: ${result.pages.length} pages imported`);
        totalPages += result.pages.length;
        successCount++;
      } catch (error) {
        spinner.fail(`${extName}: failed`);
        const message = error instanceof Error ? error.message : String(error);
        printError(`  ${message}`);
      }
    }

    console.log();
    printSuccess(`Imported ${totalPages} pages from ${successCount} extension(s)`);
  });
}

// ============================================================================
// Import technical documentation
// ============================================================================

export interface DocsImportTechnicalOptions {
  subpages?: boolean;
  hooks?: boolean;
  config?: boolean;
  api?: boolean;
  limit?: string;
}

export async function docsImportTechnicalCommand(
  pages: string[],
  options: DocsImportTechnicalOptions
): Promise<void> {
  await withContext(async (ctx) => {
    const limit = parseInt(options.limit || '100', 10);

    // Determine what to import
    interface ImportTask {
      docType: TechnicalDocType;
      pageTitle?: string;
      includeSubpages: boolean;
    }

    const tasks: ImportTask[] = [];

    // If specific pages are given, import those
    if (pages.length > 0) {
      for (const page of pages) {
        // Determine doc type from page prefix
        let docType: TechnicalDocType = 'manual';
        if (page.startsWith('Manual:Hooks')) {
          docType = 'hooks';
        } else if (page.startsWith('Manual:$wg')) {
          docType = 'config';
        } else if (page.startsWith('API:')) {
          docType = 'api';
        }

        tasks.push({
          docType,
          pageTitle: page,
          includeSubpages: options.subpages || false,
        });
      }
    }

    // If type flags are given, import those types
    if (options.hooks) {
      tasks.push({ docType: 'hooks', includeSubpages: true });
    }
    if (options.config) {
      tasks.push({ docType: 'config', includeSubpages: true });
    }
    if (options.api) {
      tasks.push({ docType: 'api', includeSubpages: true });
    }

    if (tasks.length === 0) {
      printError('No documentation specified. Use:');
      console.log('  docs import-technical Manual:Hooks [--subpages]');
      console.log('  docs import-technical --hooks    # All hook documentation');
      console.log('  docs import-technical --config   # All config variable docs');
      console.log('  docs import-technical --api      # All API documentation');
      process.exit(1);
    }

    console.log(chalk.bold(`\nImporting technical documentation...\n`));

    let totalDocs = 0;

    for (const task of tasks) {
      const label = task.pageTitle || TECHNICAL_DOC_TYPES[task.docType].mainPage;
      const spinner = ora(`Importing ${label}...`).start();

      try {
        const docs = await fetchTechnicalDocs(task.docType, {
          pageTitle: task.pageTitle,
          includeSubpages: task.includeSubpages,
          limit,
          onProgress: (current, total, message) => {
            spinner.text = `${label}: ${message || `${current}/${total}`}`;
          },
        });

        // Store in database
        for (const doc of docs) {
          ctx.db.upsertTechnicalDoc({
            docType: doc.docType,
            pageTitle: doc.pageTitle,
            localPath: doc.localPath,
            content: doc.content,
            contentHash: doc.contentHash,
            expiresAt: doc.expiresAt,
          });

          // Index for FTS
          ctx.db.indexPage('technical', doc.pageTitle, doc.content);
        }

        spinner.succeed(`${label}: ${docs.length} pages imported`);
        totalDocs += docs.length;
      } catch (error) {
        spinner.fail(`${label}: failed`);
        const message = error instanceof Error ? error.message : String(error);
        printError(`  ${message}`);
      }
    }

    console.log();
    printSuccess(`Imported ${totalDocs} technical documentation pages`);
  });
}

// ============================================================================
// List documentation
// ============================================================================

export interface DocsListOptions {
  outdated?: boolean;
  type?: string;
}

export async function docsListCommand(options: DocsListOptions): Promise<void> {
  await withContext(async (ctx) => {
    const stats = ctx.db.getDocsStats();

    if (options.outdated) {
      // Show only outdated docs
      const outdated = ctx.db.getOutdatedDocs();

      if (outdated.extensions.length === 0 && outdated.technical.length === 0) {
        printInfo('No outdated documentation found');
        return;
      }

      console.log(chalk.bold('\nOutdated Documentation\n'));

      if (outdated.extensions.length > 0) {
        console.log(chalk.yellow('Extensions:'));
        for (const ext of outdated.extensions) {
          console.log(`  ${ext.extensionName} (expired ${ext.expiresAt})`);
        }
      }

      if (outdated.technical.length > 0) {
        console.log(chalk.yellow('\nTechnical:'));
        for (const doc of outdated.technical) {
          console.log(`  [${doc.docType}] ${doc.pageTitle} (expired ${doc.expiresAt})`);
        }
      }

      console.log();
      printWarning(`Run 'docs update' to refresh outdated documentation`);
      return;
    }

    // Show summary
    console.log(chalk.bold('\nDocumentation Summary\n'));

    const summaryTable = new Table({
      head: ['Type', 'Count', 'Details'],
      style: { head: ['cyan'] },
    });

    summaryTable.push(
      ['Extensions', String(stats.extensionCount), `${stats.extensionPagesCount} total pages`],
      ['Technical', String(stats.technicalCount), Object.entries(stats.technicalByType)
        .map(([k, v]) => `${k}: ${v}`)
        .join(', ') || 'none']
    );

    console.log(summaryTable.toString());

    // List extensions
    const extensions = ctx.db.getExtensionDocs();
    if (extensions.length > 0) {
      console.log(chalk.bold('\nExtension Documentation\n'));

      const extTable = new Table({
        head: ['Extension', 'Version', 'Pages', 'Status'],
        style: { head: ['cyan'] },
      });

      for (const ext of extensions) {
        const status = ext.expiresAt
          ? (isExpired(ext.expiresAt) ? chalk.red('expired') : chalk.green(formatExpiration(ext.expiresAt)))
          : chalk.dim('no expiry');

        extTable.push([
          ext.extensionName,
          ext.version || '-',
          String(ext.pagesCount),
          status,
        ]);
      }

      console.log(extTable.toString());
    }

    // List technical docs by type
    const technicalDocs = ctx.db.getTechnicalDocs(options.type);
    if (technicalDocs.length > 0 && !options.type) {
      console.log(chalk.bold('\nTechnical Documentation\n'));

      // Group by type
      const byType = new Map<string, number>();
      for (const doc of technicalDocs) {
        byType.set(doc.docType, (byType.get(doc.docType) || 0) + 1);
      }

      const techTable = new Table({
        head: ['Type', 'Pages'],
        style: { head: ['cyan'] },
      });

      for (const [type, count] of byType) {
        techTable.push([type, String(count)]);
      }

      console.log(techTable.toString());
      console.log(chalk.dim('\nUse --type=<type> to see specific pages'));
    } else if (technicalDocs.length > 0 && options.type) {
      console.log(chalk.bold(`\n${options.type} Documentation\n`));

      for (const doc of technicalDocs.slice(0, 50)) {
        const status = doc.expiresAt && isExpired(doc.expiresAt) ? chalk.red(' (expired)') : '';
        console.log(`  ${doc.pageTitle}${status}`);
      }

      if (technicalDocs.length > 50) {
        console.log(chalk.dim(`  ... and ${technicalDocs.length - 50} more`));
      }
    }
  });
}

// ============================================================================
// Update documentation
// ============================================================================

export async function docsUpdateCommand(): Promise<void> {
  await withContext(async (ctx) => {
    const outdated = ctx.db.getOutdatedDocs();

    if (outdated.extensions.length === 0 && outdated.technical.length === 0) {
      printInfo('All documentation is up to date');
      return;
    }

    console.log(chalk.bold('\nUpdating outdated documentation...\n'));

    // Update extensions
    for (const ext of outdated.extensions) {
      const spinner = ora(`Updating ${ext.extensionName}...`).start();

      try {
        const result = await fetchExtensionDocs(ext.extensionName, {
          onProgress: (current, total, message) => {
            spinner.text = `${ext.extensionName}: ${message || `${current}/${total}`}`;
          },
        });

        const docId = ctx.db.upsertExtensionDoc({
          extensionName: result.info.extensionName,
          version: result.info.version,
          pagesCount: result.info.pagesCount,
          expiresAt: result.info.expiresAt,
        });

        for (const page of result.pages) {
          ctx.db.upsertExtensionDocPage({
            extensionId: docId,
            pageTitle: page.pageTitle,
            localPath: page.localPath,
            content: page.content,
            contentHash: page.contentHash,
          });
          ctx.db.indexPage('extension', page.pageTitle, page.content);
        }

        spinner.succeed(`${ext.extensionName}: ${result.pages.length} pages updated`);
      } catch (error) {
        spinner.fail(`${ext.extensionName}: failed`);
        const message = error instanceof Error ? error.message : String(error);
        printError(`  ${message}`);
      }
    }

    // Update technical docs (by unique doc type)
    const typesToUpdate = new Set(outdated.technical.map(d => d.docType));
    for (const docType of typesToUpdate) {
      const spinner = ora(`Updating ${docType} documentation...`).start();

      try {
        const docs = await fetchTechnicalDocs(docType as TechnicalDocType, {
          includeSubpages: true,
          onProgress: (current, total, message) => {
            spinner.text = `${docType}: ${message || `${current}/${total}`}`;
          },
        });

        for (const doc of docs) {
          ctx.db.upsertTechnicalDoc({
            docType: doc.docType,
            pageTitle: doc.pageTitle,
            localPath: doc.localPath,
            content: doc.content,
            contentHash: doc.contentHash,
            expiresAt: doc.expiresAt,
          });
          ctx.db.indexPage('technical', doc.pageTitle, doc.content);
        }

        spinner.succeed(`${docType}: ${docs.length} pages updated`);
      } catch (error) {
        spinner.fail(`${docType}: failed`);
        const message = error instanceof Error ? error.message : String(error);
        printError(`  ${message}`);
      }
    }

    console.log();
    printSuccess('Documentation update complete');
  });
}

// ============================================================================
// Remove documentation
// ============================================================================

export async function docsRemoveCommand(target: string): Promise<void> {
  await withContext(async (ctx) => {
    // Check if it's an extension
    const extName = target.replace(/^Extension:/, '');
    const ext = ctx.db.getExtensionDoc(extName);

    if (ext) {
      const deleted = ctx.db.deleteExtensionDoc(extName);
      if (deleted) {
        printSuccess(`Removed documentation for extension: ${extName}`);
      } else {
        printError(`Failed to remove documentation for: ${extName}`);
      }
      return;
    }

    // Check if it's a technical doc type
    const validTypes = Object.keys(TECHNICAL_DOC_TYPES);
    if (validTypes.includes(target)) {
      const count = ctx.db.deleteTechnicalDocsByType(target);
      printSuccess(`Removed ${count} ${target} documentation pages`);
      return;
    }

    // Try to match a specific technical doc
    for (const docType of validTypes) {
      const doc = ctx.db.getTechnicalDoc(docType, target);
      if (doc) {
        ctx.db.deleteTechnicalDoc(docType, target);
        printSuccess(`Removed technical doc: ${target}`);
        return;
      }
    }

    printError(`Documentation not found: ${target}`);
    console.log('Specify an extension name (e.g., CirrusSearch) or doc type (e.g., hooks)');
  });
}

// ============================================================================
// Search documentation
// ============================================================================

export interface DocsSearchOptions {
  tier?: string;
  limit?: string;
}

export async function docsSearchCommand(query: string, options: DocsSearchOptions): Promise<void> {
  await withContext(async (ctx) => {
    const limit = parseInt(options.limit || '20', 10);

    console.log(chalk.bold(`\nSearching for: ${query}\n`));

    const results = ctx.db.searchFts(query, {
      tier: options.tier,
      limit,
    });

    if (results.length === 0) {
      printInfo('No results found');
      return;
    }

    // Group by tier
    const byTier = new Map<string, typeof results>();
    for (const result of results) {
      const tierResults = byTier.get(result.tier) || [];
      tierResults.push(result);
      byTier.set(result.tier, tierResults);
    }

    for (const [tier, tierResults] of byTier) {
      console.log(chalk.cyan(`\n${tier.toUpperCase()} (${tierResults.length})\n`));

      for (const result of tierResults) {
        console.log(chalk.bold(`  ${result.title}`));
        // Clean up snippet (remove HTML)
        const cleanSnippet = result.snippet
          .replace(/<mark>/g, chalk.yellow.bold(''))
          .replace(/<\/mark>/g, chalk.reset(''))
          .replace(/\.\.\./g, '...');
        console.log(chalk.dim(`    ${cleanSnippet}`));
        console.log();
      }
    }

    console.log(chalk.dim(`Found ${results.length} results`));
  });
}
