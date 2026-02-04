/**
 * diff command - Show local changes
 */

import chalk from 'chalk';
import { withContext } from '../utils/context.js';
import { printInfo, printSection, formatChange, CHANGE_COLORS, CHANGE_SYMBOLS } from '../utils/format.js';

export interface DiffOptions {
  templates?: boolean;
  verbose?: boolean;
}

export async function diffCommand(options: DiffOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      const changes = ctx.engine.getChanges({ includeTemplates: options.templates });

      if (changes.length === 0) {
        printInfo('No local changes');
        return;
      }

      // Group changes by type
      const grouped: Record<string, typeof changes> = {};
      for (const change of changes) {
        if (!grouped[change.type]) {
          grouped[change.type] = [];
        }
        grouped[change.type].push(change);
      }

      // Display order
      const typeOrder = ['new_local', 'modified_local', 'deleted_local', 'conflict'];
      const typeLabels: Record<string, string> = {
        new_local: 'New (local only)',
        modified_local: 'Modified',
        deleted_local: 'Deleted locally',
        conflict: 'Conflicts',
      };

      for (const type of typeOrder) {
        const group = grouped[type];
        if (!group || group.length === 0) continue;

        const label = typeLabels[type] || type;
        const color = CHANGE_COLORS[type] || chalk.white;
        printSection(color(label));

        for (const change of group) {
          console.log(formatChange(change.type, change.title, change.filepath));

          if (options.verbose && change.type === 'modified_local') {
            // Show hash diff
            if (change.localHash && change.dbHash) {
              console.log(chalk.dim(`    local:  ${change.localHash}`));
              console.log(chalk.dim(`    synced: ${change.dbHash}`));
            }
          }

          if (options.verbose && change.type === 'conflict') {
            if (change.wikiTimestamp) {
              console.log(chalk.dim(`    wiki modified: ${change.wikiTimestamp}`));
            }
            if (change.dbTimestamp) {
              console.log(chalk.dim(`    last synced:   ${change.dbTimestamp}`));
            }
          }
        }
      }

      // Summary
      console.log();
      const summary: string[] = [];
      if (grouped.new_local?.length) {
        summary.push(chalk.green(`${grouped.new_local.length} new`));
      }
      if (grouped.modified_local?.length) {
        summary.push(chalk.yellow(`${grouped.modified_local.length} modified`));
      }
      if (grouped.deleted_local?.length) {
        summary.push(chalk.magenta(`${grouped.deleted_local.length} deleted`));
      }
      if (grouped.conflict?.length) {
        summary.push(chalk.red(`${grouped.conflict.length} conflicts`));
      }

      console.log(`Summary: ${summary.join(', ')}`);

      // Show legend
      console.log();
      console.log(chalk.dim('Legend:'));
      console.log(chalk.dim(`  [N] New   [M] Modified   [D] Deleted   [C] Conflict`));
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(chalk.red('Error:'), message);
    process.exit(1);
  }
}
