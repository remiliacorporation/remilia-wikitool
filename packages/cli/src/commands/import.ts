/**
 * import command - Data import helpers
 */

import chalk from 'chalk';
import ora from 'ora';
import { importToCargo, type ImportResult, type ImportSource } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import { printError, printInfo, printSection, printSuccess, printWarning } from '../utils/format.js';

export interface ImportCommandOptions {
  table?: string;
  template?: string;
  titleField?: string;
  titlePrefix?: string;
  category?: string;
  mode?: string;
  type?: string;
  write?: boolean;
  format?: string;
  articleHeader?: boolean;
  meta?: boolean;
}

export async function importCargoCommand(path: string, options: ImportCommandOptions = {}): Promise<void> {
  const tableName = options.table;
  if (!tableName) {
    printError('Missing required option: --table <name>');
    process.exit(1);
  }

  const sourceType = resolveSourceType(path, options.type);
  if (!sourceType) {
    printError('Unable to determine import type (use --type csv|json)');
    process.exit(1);
  }

  const spinner = ora('Preparing import...').start();

  try {
    await withContext(async (ctx) => {
      const source: ImportSource = { type: sourceType, path };
      const result = await importToCargo(source, {
        tableName,
        templateName: options.template,
        titleField: options.titleField,
        titlePrefix: options.titlePrefix,
        updateMode: normalizeMode(options.mode),
        categoryName: options.category,
        articleHeader: options.articleHeader,
        write: options.write,
      }, { fs: ctx.fs });

      spinner.stop();

      const format = (options.format || 'text').toLowerCase();
      if (format === 'json') {
        const output = options.meta === false ? result : withMeta(result, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      printImportSummary(result);

      if (!options.write) {
        printWarning('Dry run only. Use --write to apply changes.');
      }
    });
  } catch (error) {
    spinner.fail('Import failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

function resolveSourceType(path: string, explicit?: string): ImportSource['type'] | null {
  if (explicit) {
    const lower = explicit.toLowerCase();
    if (lower === 'csv' || lower === 'json') return lower;
  }

  const lower = path.toLowerCase();
  if (lower.endsWith('.csv')) return 'csv';
  if (lower.endsWith('.json')) return 'json';
  return null;
}

function normalizeMode(mode?: string): 'create' | 'update' | 'upsert' {
  const lower = (mode || 'create').toLowerCase();
  if (lower === 'update' || lower === 'upsert') return lower;
  return 'create';
}

function printImportSummary(result: ImportResult): void {
  printSection('Import Summary');
  console.log(`  Created: ${result.pagesCreated.length}`);
  console.log(`  Updated: ${result.pagesUpdated.length}`);
  console.log(`  Skipped: ${result.pagesSkipped.length}`);
  console.log(`  Errors: ${result.errors.length}`);

  if (result.errors.length > 0) {
    printSection('Errors');
    for (const error of result.errors.slice(0, 10)) {
      printError(`Row ${error.row}: ${error.message}${error.title ? ` (${error.title})` : ''}`);
    }
    if (result.errors.length > 10) {
      printInfo(`... and ${result.errors.length - 10} more`);
    }
  }

  if (result.pages.length > 0) {
    printSection('Pages');
    for (const page of result.pages.slice(0, 10)) {
      const label = page.action === 'create' ? chalk.green('CREATE')
        : page.action === 'update' ? chalk.yellow('UPDATE')
          : chalk.dim('SKIP');
      console.log(`  ${label} ${page.title}`);
    }
    if (result.pages.length > 10) {
      printInfo(`... and ${result.pages.length - 10} more`);
    }
  } else {
    printSuccess('No pages generated');
  }
}
