/**
 * push command - Upload local changes to wiki
 */

import chalk from 'chalk';
import ora from 'ora';
import { Namespace, getNamespaceFromTitle, lintLuaContent, isSeleneAvailable, type LuaLintError } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printInfo, printWarning, printSection, formatChange } from '../utils/format.js';

export interface PushOptions {
  summary: string;
  dryRun?: boolean;
  force?: boolean;
  delete?: boolean;
  templates?: boolean;
  categories?: boolean;
  lint?: boolean;
  lintStrict?: boolean;
}

export async function pushCommand(options: PushOptions): Promise<void> {
  const mode = options.dryRun ? 'DRY RUN' : 'Push';
  console.log(chalk.bold(`${mode}: Pushing to wiki`));

  if (options.dryRun) {
    printInfo('This is a dry run - no changes will be made');
  }

  const spinner = ora('Checking for changes...').start();

  try {
    await withContext(async (ctx) => {
      // Determine namespace filter
      const namespaces = options.categories ? [Namespace.Category] : undefined;

      // First, get the list of changes to show what we'll push
      const changes = ctx.engine.getChanges({
        includeTemplates: options.templates,
        namespaces,
      });

      const pushable = changes.filter(c => c.type === 'modified_local' || c.type === 'new_local');
      const deletions = changes.filter(c => c.type === 'deleted_local');
      const totalChanges = pushable.length + (options.delete ? deletions.length : 0);

      if (deletions.length > 0 && !options.delete) {
        printWarning(`Detected ${deletions.length} local deletions (use --delete to sync deletes to the wiki)`);
      }

      if (totalChanges === 0) {
        spinner.stop();
        printInfo('No local changes to push');
        return;
      }

      if (options.lint !== false && isSeleneAvailable()) {
        const modulePages = pushable.filter(p => getNamespaceFromTitle(p.title) === Namespace.Module);
        if (modulePages.length > 0) {
          spinner.text = 'Linting Lua modules...';
          const lintResults = [];

          for (const page of modulePages) {
            const file = ctx.fs.readFile(page.filepath);
            const content = file?.content ?? '';
            lintResults.push(await lintLuaContent(content, page.title));
          }

          const errors = lintResults.flatMap(r => r.errors);
          const warnings = lintResults.flatMap(r => r.warnings);

          if (errors.length > 0) {
            printSection('Lua lint errors');
            for (const issue of errors) {
              printError(formatLintIssue(issue));
            }
            if (!options.force) {
              throw new Error('Lua lint errors found (use --force to push anyway)');
            }
          }

          if (warnings.length > 0 && options.lintStrict) {
            printSection('Lua lint warnings');
            for (const issue of warnings) {
              printWarning(formatLintIssue(issue));
            }
            if (!options.force) {
              throw new Error('Lua lint warnings found (use --force to push anyway)');
            }
          }
        }
      }

      spinner.text = `Found ${totalChanges} changes to push`;

      // For dry run, just show what would be pushed
      if (options.dryRun) {
        spinner.stop();

        printSection(`Would push ${totalChanges} pages`);

        const newPages = pushable.filter(c => c.type === 'new_local');
        const modified = pushable.filter(c => c.type === 'modified_local');

        if (newPages.length > 0) {
          console.log(chalk.green(`  New: ${newPages.length}`));
          for (const page of newPages) {
            console.log(formatChange('new_local', page.title, page.filepath));
          }
        }

        if (modified.length > 0) {
          console.log(chalk.yellow(`  Modified: ${modified.length}`));
          for (const page of modified) {
            console.log(formatChange('modified_local', page.title, page.filepath));
          }
        }

        if (options.delete && deletions.length > 0) {
          console.log(chalk.magenta(`  Deleted: ${deletions.length}`));
          for (const page of deletions) {
            console.log(formatChange('deleted_local', page.title, page.filepath));
          }
        }

        console.log();
        console.log(`Summary: ${newPages.length} new, ${modified.length} modified, ${options.delete ? deletions.length : 0} deleted`);
        console.log(`Edit summary: "${options.summary}"`);
        console.log();
        printInfo('To execute, run without --dry-run');
        return;
      }

      // Actual push requires authentication
      spinner.text = 'Authenticating...';
    }, { requireAuth: false }); // First pass to check changes

    // Now do the actual push with authentication
    await withContext(async (ctx) => {
      const spinner = ora('Pushing changes...').start();

      // Determine namespace filter
      const namespaces = options.categories ? [Namespace.Category] : undefined;

      const result = await ctx.engine.push({
        summary: options.summary,
        dryRun: options.dryRun,
        force: options.force,
        delete: options.delete,
        includeTemplates: options.templates,
        namespaces,
        onProgress: (message, current, total) => {
          if (current !== undefined && total !== undefined) {
            spinner.text = `${message} (${current}/${total})`;
          } else {
            spinner.text = message;
          }
        },
      });

      spinner.stop();

      // Handle conflicts
      if (result.conflicts.length > 0) {
        printSection('Conflicts detected');
        printWarning(`Wiki has newer changes for ${result.conflicts.length} pages:`);
        for (const title of result.conflicts) {
          console.log(chalk.red(`  ${title}`));
        }
        console.log();
        printInfo('Run `bun run wikitool pull` to get latest changes, or use --force to overwrite.');
      }

      // Show results
      if (result.pushed > 0) {
        printSection('Pushed');
        const pushed = result.pages.filter(p => p.action === 'pushed' || p.action === 'created' || p.action === 'deleted');
        for (const page of pushed.slice(0, 15)) {
          const symbol = page.action === 'created' ? 'N' : (page.action === 'deleted' ? 'D' : 'M');
          console.log(`  [${symbol}] ${page.title}`);
        }
        if (pushed.length > 15) {
          console.log(chalk.dim(`    ... and ${pushed.length - 15} more`));
        }
      }

      if (result.unchanged > 0) {
        printSection('Unchanged');
        const unchanged = result.pages.filter(p => p.action === 'unchanged');
        for (const page of unchanged.slice(0, 10)) {
          console.log(chalk.dim(`  [=] ${page.title}`));
        }
        if (unchanged.length > 10) {
          console.log(chalk.dim(`    ... and ${unchanged.length - 10} more`));
        }
        printInfo(`${result.unchanged} pages had identical content on wiki`);
      }

      if (result.errors.length > 0) {
        printSection('Errors');
        for (const error of result.errors) {
          printError(error);
        }
      }

      console.log();
      if (result.success) {
        const parts = [];
        if (result.pushed > 0) parts.push(`${result.pushed} pushed`);
        if (result.unchanged > 0) parts.push(`${result.unchanged} unchanged`);
        printSuccess(parts.length > 0 ? `Done: ${parts.join(', ')}` : 'No changes to push');
      } else if (result.conflicts.length > 0) {
        printWarning(`Push blocked: ${result.conflicts.length} conflicts`);
        process.exit(1);
      } else {
        printError(`Push failed with ${result.errors.length} errors`);
        process.exit(1);
      }
    }, { requireAuth: true });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);

    // Handle authentication errors specially
    if (message.includes('WIKI_BOT_USER') || message.includes('WIKI_BOT_PASS')) {
      printError('Authentication required for push');
      printInfo('Set WIKI_BOT_USER and WIKI_BOT_PASS environment variables');
      process.exit(1);
    }

    printError(message);
    process.exit(1);
  }
}

function formatLintIssue(issue: LuaLintError): string {
  return `${issue.line}:${issue.column} ${issue.code} ${issue.message}`;
}

