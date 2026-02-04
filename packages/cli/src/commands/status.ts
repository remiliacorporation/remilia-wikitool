/**
 * status command - Show sync status
 */

import chalk from 'chalk';
import Table from 'cli-table3';
import { Namespace } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printSection, formatNamespace, formatSyncStatus, formatTime } from '../utils/format.js';

export interface StatusOptions {
  modified?: boolean;
  conflicts?: boolean;
  templates?: boolean;
}

export async function statusCommand(options: StatusOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      const status = ctx.engine.getStatus({ includeTemplates: options.templates });

      // Database stats
      const stats = ctx.db.getStats();
      const getLastPull = (namespaces: number[], legacyKey: string): string | null => {
        const key = `last_pull_ns_${[...new Set(namespaces)].sort((a, b) => a - b).join('_')}`;
        return ctx.db.getConfig(key) ?? ctx.db.getConfig(legacyKey);
      };

      const lastPull = getLastPull([Namespace.Main], 'last_article_pull');
      const lastTemplatePull = getLastPull(
        [Namespace.Template, Namespace.Module, Namespace.MediaWiki],
        'last_template_pull'
      );

      // Header
      console.log(chalk.bold('Wiki Sync Status'));
      console.log();

      // Overview table
      const overviewTable = new Table({
        chars: { mid: '', 'left-mid': '', 'mid-mid': '', 'right-mid': '' },
        style: { 'padding-left': 2, 'padding-right': 2 },
      });

      overviewTable.push(
        ['Total pages:', chalk.cyan(stats.totalPages.toString())],
        ['Synced:', chalk.green(status.synced.toString())],
        ['Modified locally:', chalk.yellow(status.modified.length.toString())],
        ['New locally:', chalk.magenta(status.newLocal.length.toString())],
        ['Conflicts:', status.conflicts.length > 0 ? chalk.red(status.conflicts.length.toString()) : '0'],
        ['Deleted locally:', chalk.dim(status.deletedLocal.length.toString())],
      );

      console.log(overviewTable.toString());

      // Last sync info
      console.log();
      console.log(`Last article pull: ${formatTime(lastPull)}`);
      if (options.templates) {
        console.log(`Last template pull: ${formatTime(lastTemplatePull)}`);
      }

      // Show modified if requested or if there are any
      if ((options.modified || (!options.conflicts)) && status.modified.length > 0) {
        printSection('Modified');
        for (const change of status.modified.slice(0, 20)) {
          console.log(`  ${chalk.yellow('[M]')} ${change.filepath}`);
        }
        if (status.modified.length > 20) {
          console.log(chalk.dim(`  ... and ${status.modified.length - 20} more`));
        }
      }

      // Show new local
      if ((options.modified || (!options.conflicts)) && status.newLocal.length > 0) {
        printSection('New (local only)');
        for (const change of status.newLocal.slice(0, 10)) {
          console.log(`  ${chalk.green('[N]')} ${change.filepath}`);
        }
        if (status.newLocal.length > 10) {
          console.log(chalk.dim(`  ... and ${status.newLocal.length - 10} more`));
        }
      }

      // Show conflicts (always if present)
      if (status.conflicts.length > 0) {
        printSection(chalk.red('Conflicts'));
        for (const change of status.conflicts) {
          console.log(`  ${chalk.red('[C]')} ${change.filepath}`);
          if (change.wikiTimestamp && change.dbTimestamp) {
            console.log(chalk.dim(`      Wiki: ${change.wikiTimestamp}`));
            console.log(chalk.dim(`      Last sync: ${change.dbTimestamp}`));
          }
        }
      }

      // Show deleted if requested
      if (options.modified && status.deletedLocal.length > 0) {
        printSection('Deleted locally');
        for (const change of status.deletedLocal.slice(0, 10)) {
          console.log(`  ${chalk.magenta('[D]')} ${change.filepath}`);
        }
        if (status.deletedLocal.length > 10) {
          console.log(chalk.dim(`  ... and ${status.deletedLocal.length - 10} more`));
        }
      }

      // By namespace breakdown
      if (Object.keys(stats.byNamespace).length > 1) {
        printSection('By Namespace');
        for (const [ns, count] of Object.entries(stats.byNamespace)) {
          if (count > 0) {
            console.log(`  ${formatNamespace(parseInt(ns))}: ${count}`);
          }
        }
      }

      // Suggestions
      if (status.modified.length > 0 || status.newLocal.length > 0) {
        console.log();
        console.log(chalk.dim('To push changes: bun run wikitool push -s "Edit summary"'));
        console.log(chalk.dim('To see diff:     bun run wikitool diff'));
      }

      if (status.conflicts.length > 0) {
        console.log();
        console.log(chalk.dim('To resolve conflicts: bun run wikitool pull (will update from wiki)'));
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(chalk.red('Error:'), message);
    process.exit(1);
  }
}

