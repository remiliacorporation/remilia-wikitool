/**
 * delete command - Delete a page from the wiki
 *
 * Features:
 * - Creates automatic backup of page content before deletion
 * - Deletes page from wiki via API
 * - Removes local file
 * - Updates database
 *
 * Requires authentication (WIKI_BOT_USER and WIKI_BOT_PASS environment variables)
 */

import chalk from 'chalk';
import ora from 'ora';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printWarning, printInfo } from '../utils/format.js';

export interface DeleteOptions {
  reason: string;
  noBackup?: boolean;
  backupDir?: string;
  dryRun?: boolean;
}

export async function deleteCommand(title: string, options: DeleteOptions): Promise<void> {
  console.log(chalk.bold(`Delete: ${title}`));
  console.log();

  if (!options.reason) {
    printError('Reason is required: --reason "..."');
    process.exit(1);
  }

  if (options.dryRun) {
    printInfo('Dry-run mode - no changes will be made');
    console.log();
  }

  const spinner = ora('Preparing...').start();

  try {
    await withContext(async (ctx) => {
      // 1. Check if page exists in database
      const page = ctx.db.getPage(title);
      if (!page) {
        spinner.fail('Page not found in database');
        printWarning(`"${title}" is not tracked locally`);
        printInfo('Try pulling the page first: wikitool pull');
        process.exit(1);
      }

      // 2. Create backup if not disabled
      if (!options.noBackup && !options.dryRun) {
        spinner.text = 'Creating backup...';

        const backupDir = options.backupDir || join(ctx.rootDir, '.wikitool', 'deleted');
        if (!existsSync(backupDir)) {
          mkdirSync(backupDir, { recursive: true });
        }

        // Get current content from wiki (in case local is outdated)
        const wikiContent = await ctx.client.getPageContent(title);
        const content = wikiContent?.content || page.content || '';

        if (content) {
          const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
          const safeTitle = title.replace(/[\/\\:*?"<>|]/g, '_');
          const filename = `${safeTitle}_${timestamp}.wiki`;
          const backupPath = join(backupDir, filename);

          writeFileSync(backupPath, content, 'utf-8');
          printSuccess(`Backup saved to ${backupPath}`);
        } else {
          printWarning('No content to backup');
        }
      }

      if (options.dryRun) {
        spinner.stop();
        console.log();
        console.log(chalk.bold('Would delete:'));
        console.log(`  Title: ${title}`);
        console.log(`  Reason: ${options.reason}`);

        const filepath = ctx.fs.titleToFilepath(title, page.is_redirect === 1);
        if (ctx.fs.fileExists(filepath)) {
          console.log(`  Local file: ${filepath}`);
        }

        console.log();
        printInfo('Use without --dry-run to actually delete');
        return;
      }

      // 3. Delete from wiki (requires authentication)
      spinner.text = 'Deleting from wiki...';

      try {
        const result = await ctx.client.deletePage(title, options.reason);
        printSuccess(`Deleted from wiki (logid: ${result.logid})`);
      } catch (error) {
        // If delete fails, it might be because we don't have permissions
        // or the page doesn't exist on wiki
        const message = error instanceof Error ? error.message : String(error);
        if (message.includes('missingtitle')) {
          printWarning('Page does not exist on wiki (already deleted?)');
        } else if (message.includes('permissiondenied')) {
          spinner.fail('Delete failed');
          printError('Permission denied - check bot credentials have delete rights');
          process.exit(1);
        } else {
          spinner.fail('Delete failed');
          printError(message);
          process.exit(1);
        }
      }

      // 4. Remove local file
      spinner.text = 'Removing local file...';
      const filepath = ctx.fs.titleToFilepath(title, page.is_redirect === 1);
      if (ctx.fs.fileExists(filepath)) {
        ctx.fs.deleteFile(filepath);
        printSuccess(`Removed local file: ${filepath}`);
      } else {
        printInfo('No local file to remove');
      }

      // 5. Update database
      spinner.text = 'Updating database...';
      ctx.db.deletePage(title);
      printSuccess('Removed from database');

      spinner.stop();
      console.log();
      printSuccess(`Deleted "${title}"`);
    }, { requireAuth: true });
  } catch (error) {
    spinner.fail('Delete failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);

    if (message.includes('WIKI_BOT_USER') || message.includes('WIKI_BOT_PASS')) {
      console.log();
      printInfo('Set WIKI_BOT_USER and WIKI_BOT_PASS environment variables');
    }

    process.exit(1);
  }
}
