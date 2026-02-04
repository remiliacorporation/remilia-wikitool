/**
 * db command - Database operations
 */

import chalk from 'chalk';
import Table from 'cli-table3';
import ora from 'ora';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printInfo, printSection, formatNamespace, formatSyncStatus } from '../utils/format.js';

export interface MigrateOptions {
  status?: boolean;
  validate?: boolean;
}

export async function dbCommand(subcommand: 'stats' | 'sync' | 'migrate', options?: MigrateOptions): Promise<void> {
  switch (subcommand) {
    case 'stats':
      await statsCommand();
      break;
    case 'sync':
      await syncCommand();
      break;
    case 'migrate':
      await migrateCommand(options);
      break;
    default:
      printError(`Unknown subcommand: ${subcommand}`);
      process.exit(1);
  }
}

async function statsCommand(): Promise<void> {
  try {
    await withContext(async (ctx) => {
      const stats = ctx.db.getStats();

      console.log(chalk.bold('Database Statistics'));
      console.log();

      // Main stats table
      const mainTable = new Table({
        head: [chalk.bold('Metric'), chalk.bold('Value')],
        style: { head: [], border: [] },
      });

      mainTable.push(
        ['Total pages', stats.totalPages.toString()],
        ['Total categories', stats.totalCategories.toString()],
      );

      console.log(mainTable.toString());

      // By namespace
      if (Object.keys(stats.byNamespace).length > 0) {
        printSection('By Namespace');
        const nsTable = new Table({
          head: [chalk.bold('Namespace'), chalk.bold('Count')],
          style: { head: [], border: [] },
        });

        for (const [ns, count] of Object.entries(stats.byNamespace)) {
          if (count > 0) {
            nsTable.push([formatNamespace(parseInt(ns)), count.toString()]);
          }
        }

        console.log(nsTable.toString());
      }

      // By status
      printSection('By Sync Status');
      const statusTable = new Table({
        head: [chalk.bold('Status'), chalk.bold('Count')],
        style: { head: [], border: [] },
      });

      for (const [status, count] of Object.entries(stats.byStatus)) {
        statusTable.push([formatSyncStatus(status), count.toString()]);
      }

      console.log(statusTable.toString());

      // By type
      if (Object.keys(stats.byType).length > 0) {
        printSection('By Page Type');
        const typeTable = new Table({
          head: [chalk.bold('Type'), chalk.bold('Count')],
          style: { head: [], border: [] },
        });

        for (const [type, count] of Object.entries(stats.byType)) {
          if (count > 0) {
            typeTable.push([type, count.toString()]);
          }
        }

        console.log(typeTable.toString());
      }

      // Configuration
      printSection('Configuration');
      const configKeys = [
        'wiki_api_url',
        'last_article_pull',
        'last_template_pull',
        'schema_version',
      ];

      for (const key of configKeys) {
        const value = ctx.db.getConfig(key);
        console.log(`  ${key}: ${chalk.cyan(value || 'not set')}`);
      }

      // Recent sync log
      const logs = ctx.db.getSyncLogs(5);
      if (logs.length > 0) {
        printSection('Recent Sync Operations');
        for (const log of logs) {
          const statusColor = log.status === 'success' ? chalk.green :
                            log.status === 'failed' ? chalk.red :
                            chalk.yellow;
          const title = log.page_title ? ` (${log.page_title})` : '';
          console.log(`  ${log.timestamp} ${statusColor(log.operation)} ${log.status}${title}`);
        }
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function syncCommand(): Promise<void> {
  console.log(chalk.bold('Syncing files with database'));

  const spinner = ora('Scanning files...').start();

  try {
    await withContext(async (ctx) => {
      // Re-scan all files and update database
      const result = await ctx.engine.initFromFiles({ includeTemplates: true });

      spinner.stop();

      if (result.errors.length > 0) {
        printSection('Errors');
        for (const error of result.errors.slice(0, 10)) {
          printError(error);
        }
        if (result.errors.length > 10) {
          printInfo(`... and ${result.errors.length - 10} more errors`);
        }
      }

      printSuccess(`Synced ${result.added} files with database`);
    });
  } catch (error) {
    spinner.fail('Sync failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function migrateCommand(options?: MigrateOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      // --validate: Validate schema
      if (options?.validate) {
        console.log(chalk.bold('Validating database schema'));
        console.log();

        const validation = ctx.db.validateSchema();

        const table = new Table({
          head: [chalk.bold('Check'), chalk.bold('Result')],
          style: { head: [], border: [] },
        });

        table.push(
          ['Current version', validation.currentVersion],
          ['Expected version', validation.expectedVersion],
          ['Schema valid', validation.valid ? chalk.green('Yes') : chalk.red('No')],
        );

        if (validation.missingTables) {
          table.push(['Missing tables', chalk.red(validation.missingTables.join(', '))]);
        }

        console.log(table.toString());

        if (!validation.valid) {
          console.log();
          printInfo('Run "wikitool db migrate" to apply pending migrations');
          process.exit(1);
        }
        return;
      }

      // --status: Show migration status
      if (options?.status) {
        console.log(chalk.bold('Migration Status'));
        console.log();

        const currentVersion = ctx.db.getSchemaVersion();
        const expectedVersion = ctx.db.getExpectedVersion();
        const history = ctx.db.getMigrationHistory();
        const pending = ctx.db.getPendingMigrations();

        // Current state
        console.log(`Current version: ${chalk.cyan(currentVersion)}`);
        console.log(`Expected version: ${chalk.cyan(expectedVersion)}`);
        console.log();

        // Applied migrations
        if (history.length > 0) {
          printSection('Applied Migrations');
          const historyTable = new Table({
            head: [chalk.bold('Version'), chalk.bold('Applied At')],
            style: { head: [], border: [] },
          });

          for (const m of history) {
            historyTable.push([chalk.green(m.version), m.applied_at]);
          }

          console.log(historyTable.toString());
        }

        // Pending migrations
        if (pending.length > 0) {
          console.log();
          printSection('Pending Migrations');
          for (const version of pending) {
            console.log(`  ${chalk.yellow(version)}`);
          }
          console.log();
          printInfo('Run "wikitool db migrate" to apply pending migrations');
        } else {
          console.log();
          printSuccess('All migrations applied');
        }
        return;
      }

      // Default: Run migrations
      console.log(chalk.bold('Running database migrations'));
      console.log();

      const pending = ctx.db.getPendingMigrations();
      if (pending.length === 0) {
        printSuccess('No pending migrations');
        console.log(`Database at schema version ${chalk.cyan(ctx.db.getSchemaVersion())}`);
        return;
      }

      console.log(`Pending migrations: ${pending.join(', ')}`);
      console.log();

      const spinner = ora('Running migrations...').start();

      const result = ctx.db.runMigrationsManual();

      spinner.stop();

      // Report results
      if (result.applied.length > 0) {
        printSection('Applied');
        for (const version of result.applied) {
          console.log(`  ${chalk.green('✓')} ${version}`);
        }
      }

      if (result.failed) {
        console.log();
        printError(`Migration ${result.failed.version} failed: ${result.failed.error}`);
        process.exit(1);
      }

      console.log();
      printSuccess(`Database migrated to version ${ctx.db.getSchemaVersion()}`);
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
