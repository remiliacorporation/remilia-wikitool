/**
 * pull command - Download pages from wiki
 */

import chalk from 'chalk';
import ora from 'ora';
import { Namespace } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printInfo, printSection, formatChange } from '../utils/format.js';

export interface PullOptions {
  full?: boolean;
  category?: string;
  templates?: boolean;
  categories?: boolean;
  all?: boolean;
  overwriteLocal?: boolean;
}

export async function pullCommand(options: PullOptions): Promise<void> {
  console.log(chalk.bold('Pulling from wiki'));

  const spinner = ora('Connecting...').start();

  try {
    await withContext(async (ctx) => {
      // Determine namespaces to pull
      let namespaces: number[] = [Namespace.Main];
      let includeTemplates = false;

      if (options.templates) {
        namespaces = [Namespace.Template, Namespace.Module, Namespace.MediaWiki];
        includeTemplates = true;
      } else if (options.categories) {
        namespaces = [Namespace.Category];
      } else if (options.all) {
        namespaces = [
          Namespace.Main,
          Namespace.Category,
          Namespace.Template,
          Namespace.Module,
          Namespace.MediaWiki,
        ];
        includeTemplates = true;
      }

      spinner.text = 'Fetching page list...';

      const result = await ctx.engine.pull({
        namespaces,
        category: options.category,
        full: options.full,
        overwriteLocal: options.overwriteLocal,
        includeTemplates,
        onProgress: (message, current, total) => {
          if (current !== undefined && total !== undefined && total > 0) {
            spinner.text = `${message} (${current}/${total})`;
          } else {
            spinner.text = message;
          }
        },
      });

      spinner.stop();

      // Show results
      if (result.pages.length === 0) {
        printInfo('No pages to pull - already up to date');
        return;
      }

      printSection('Results');

      // Group by action
      const created = result.pages.filter(p => p.action === 'created');
      const updated = result.pages.filter(p => p.action === 'updated');
      const skipped = result.pages.filter(p => p.action === 'skipped');
      const errors = result.pages.filter(p => p.action === 'error');

      if (created.length > 0) {
        console.log(chalk.green(`  Created: ${created.length}`));
        for (const page of created.slice(0, 10)) {
          console.log(formatChange('new_local', page.title));
        }
        if (created.length > 10) {
          console.log(chalk.dim(`    ... and ${created.length - 10} more`));
        }
      }

      if (updated.length > 0) {
        console.log(chalk.yellow(`  Updated: ${updated.length}`));
        for (const page of updated.slice(0, 10)) {
          console.log(formatChange('modified_local', page.title));
        }
        if (updated.length > 10) {
          console.log(chalk.dim(`    ... and ${updated.length - 10} more`));
        }
      }

      if (skipped.length > 0) {
        console.log(chalk.dim(`  Skipped (unchanged): ${skipped.length}`));
      }

      if (errors.length > 0) {
        console.log(chalk.red(`  Errors: ${errors.length}`));
        for (const page of errors.slice(0, 5)) {
          printError(`${page.title}: ${page.error}`);
        }
      }

      console.log();
      if (result.success) {
        printSuccess(`Pulled ${result.pulled} pages`);
      } else {
        printError(`Pull completed with ${result.errors.length} errors`);
      }
    });
  } catch (error) {
    spinner.fail('Pull failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
