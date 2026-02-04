/**
 * index command - Index operations (rebuild, stats, backlinks, orphans)
 */

import chalk from 'chalk';
import Table from 'cli-table3';
import ora from 'ora';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printSection, printInfo, printWarning } from '../utils/format.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import {
  rebuildIndex,
  getIndexStats,
  getBacklinks,
  getOrphanPages,
  getTopTemplates,
  getTopCategories,
  getTopLinkedPages,
  isIndexBuilt,
  getEmptyCategories,
  pruneEmptyCategories,
} from '@wikitool/core';

export type IndexSubcommand = 'rebuild' | 'stats' | 'backlinks' | 'orphans' | 'prune-categories';

export interface IndexOptions {
  limit?: string;
  verbose?: boolean;
  apply?: boolean;
  minMembers?: string;
  json?: boolean;
  meta?: boolean;
}

export async function indexCommand(
  subcommand: IndexSubcommand,
  arg?: string,
  options: IndexOptions = {}
): Promise<void> {
  switch (subcommand) {
    case 'rebuild':
      await rebuildCommand(options);
      break;
    case 'stats':
      await statsCommand(options);
      break;
    case 'backlinks':
      if (!arg) {
        printError('Missing required argument: <title>');
        console.log();
        console.log('Usage: wikitool index backlinks <title>');
        process.exit(1);
      }
      await backlinksCommand(arg, options);
      break;
    case 'orphans':
      await orphansCommand(options);
      break;
    case 'prune-categories':
      await pruneCategoriesCommand(options);
      break;
    default:
      printError(`Unknown subcommand: ${subcommand}`);
      process.exit(1);
  }
}

async function rebuildCommand(options: IndexOptions): Promise<void> {
  console.log(chalk.bold('Rebuilding index'));
  console.log();

  const spinner = ora('Processing pages...').start();
  let lastReported = 0;

  try {
    await withContext(async (ctx) => {
      const result = rebuildIndex(ctx.db, {
        onProgress: (processed, total) => {
          // Update spinner every 50 pages
          if (processed - lastReported >= 50 || processed === total) {
            spinner.text = `Processing pages... ${processed}/${total}`;
            lastReported = processed;
          }
        },
      });

      spinner.stop();

      const stats = getIndexStats(ctx.db);

      // Report results
      const table = new Table({
        head: [chalk.bold('Metric'), chalk.bold('Count')],
        style: { head: [], border: [] },
      });

      table.push(
        ['Pages processed', result.pagesProcessed.toString()],
        ['Links stored', result.linksStored.toString()],
        ['Category assignments stored', result.categoriesStored.toString()],
        ['Categories with members', stats.categoriesWithMembers.toString()],
        ['Template invocations tracked', result.templatesStored.toString()],
        ['Unique templates', stats.uniqueTemplates.toString()],
        ['Redirects mapped', result.redirectsMapped.toString()],
        ['Metadata updated', result.metadataUpdated.toString()]
      );

      console.log(table.toString());

      if (result.errors.length > 0) {
        console.log();
        printSection('Errors');
        for (const error of result.errors.slice(0, 10)) {
          printError(error);
        }
        if (result.errors.length > 10) {
          printInfo(`... and ${result.errors.length - 10} more errors`);
        }
      }

      console.log();
      printSuccess('Index rebuild complete');
    });
  } catch (error) {
    spinner.fail('Rebuild failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function statsCommand(options: IndexOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      // Check if index is built
      if (!isIndexBuilt(ctx.db)) {
        printWarning('Index has not been built yet');
        console.log();
        printInfo('Run "wikitool index rebuild" to build the index');
        return;
      }

      console.log(chalk.bold('Index Statistics'));
      console.log();

      const stats = getIndexStats(ctx.db);

      if (options.json) {
        const output = options.meta === false ? stats : withMeta(stats, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      // Main stats table
      const mainTable = new Table({
        head: [chalk.bold('Metric'), chalk.bold('Count')],
        style: { head: [], border: [] },
      });

      mainTable.push(
        ['Internal links', stats.totalLinks.toString()],
        ['Interwiki links', stats.interwikiLinks.toString()],
        ['Redirects', stats.totalRedirects.toString()],
        ['Template usages', stats.totalTemplateUsages.toString()],
        ['Unique templates', stats.uniqueTemplates.toString()],
        ['Category assignments', stats.totalCategoryAssignments.toString()],
        ['Categories with members', stats.categoriesWithMembers.toString()],
        ['Orphan pages', stats.orphanCount.toString()],
        ['Sections indexed', stats.totalSections.toString()],
        ['Template calls', stats.totalTemplateCalls.toString()],
        ['Template params', stats.totalTemplateParams.toString()],
        ['Infobox entries', stats.totalInfoboxEntries.toString()],
        ['Template metadata', stats.totalTemplateMetadata.toString()],
        ['Module deps', stats.totalModuleDeps.toString()],
        ['Cargo tables', stats.totalCargoTables.toString()],
        ['Cargo stores', stats.totalCargoStores.toString()],
        ['Cargo queries', stats.totalCargoQueries.toString()]
      );

      console.log(mainTable.toString());

      // Verbose mode: show top templates, categories, linked pages
      if (options.verbose) {
        // Top templates
        printSection('Most Used Templates');
        const topTemplates = getTopTemplates(ctx.db, 10);
        if (topTemplates.length > 0) {
          const templateTable = new Table({
            head: [chalk.bold('Template'), chalk.bold('Uses')],
            style: { head: [], border: [] },
          });
          for (const t of topTemplates) {
            templateTable.push([t.name, t.usageCount.toString()]);
          }
          console.log(templateTable.toString());
        } else {
          console.log(chalk.dim('  No templates found'));
        }

        // Top categories
        printSection('Largest Categories');
        const topCategories = getTopCategories(ctx.db, 10);
        if (topCategories.length > 0) {
          const categoryTable = new Table({
            head: [chalk.bold('Category'), chalk.bold('Members')],
            style: { head: [], border: [] },
          });
          for (const c of topCategories) {
            categoryTable.push([c.name, c.memberCount.toString()]);
          }
          console.log(categoryTable.toString());
        } else {
          console.log(chalk.dim('  No categories found'));
        }

        // Most linked pages
        printSection('Most Linked Pages');
        const topLinked = getTopLinkedPages(ctx.db, 10);
        if (topLinked.length > 0) {
          const linkedTable = new Table({
            head: [chalk.bold('Page'), chalk.bold('Incoming Links')],
            style: { head: [], border: [] },
          });
          for (const p of topLinked) {
            linkedTable.push([p.title, p.incomingLinks.toString()]);
          }
          console.log(linkedTable.toString());
        } else {
          console.log(chalk.dim('  No linked pages found'));
        }
      } else {
        console.log();
        printInfo('Use --verbose for detailed breakdowns');
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function pruneCategoriesCommand(options: IndexOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      const minMembers = parseInt(options.minMembers || '1', 10);
      const limit = parseInt(options.limit || '50', 10);
      const empty = getEmptyCategories(ctx.db, {
        minMembers: Number.isFinite(minMembers) ? minMembers : 1,
      });

      console.log(chalk.bold('Empty Categories'));
      console.log();

      if (empty.length === 0) {
        printSuccess('No empty categories found');
        return;
      }

      const table = new Table({
        head: [chalk.bold('Category'), chalk.bold('Members')],
        style: { head: [], border: [] },
      });

      for (const entry of empty.slice(0, limit)) {
        table.push([entry.name, entry.memberCount.toString()]);
      }

      console.log(table.toString());

      if (empty.length > limit) {
        console.log();
        printInfo(`Showing ${limit} of ${empty.length} categories (use --limit to show more)`);
      }

      if (options.apply) {
        const result = pruneEmptyCategories(ctx.db, {
          minMembers: Number.isFinite(minMembers) ? minMembers : 1,
          apply: true,
        });
        console.log();
        printSuccess(`Removed ${result.removed} empty categories`);
      } else {
        console.log();
        printWarning('Dry run only. Use --apply to delete empty categories.');
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function backlinksCommand(title: string, options: IndexOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      // Check if index is built
      if (!isIndexBuilt(ctx.db)) {
        printWarning('Index has not been built yet');
        console.log();
        printInfo('Run "wikitool index rebuild" to build the index');
        return;
      }

      const backlinks = getBacklinks(ctx.db, title);
      const limit = parseInt(options.limit || '50', 10);
      const limited = backlinks.slice(0, limit);

      console.log(chalk.bold(`Backlinks to "${title}"`));
      console.log();

      if (limited.length === 0) {
        console.log(chalk.dim('No pages link to this title'));
        return;
      }

      const table = new Table({
        head: [chalk.bold('Page'), chalk.bold('Link Type')],
        style: { head: [], border: [] },
      });

      for (const link of limited) {
        table.push([link.title, link.linkType]);
      }

      console.log(table.toString());

      if (backlinks.length > limit) {
        console.log();
        printInfo(`Showing ${limit} of ${backlinks.length} results (use --limit to show more)`);
      } else {
        console.log();
        console.log(chalk.dim(`${backlinks.length} page(s) link to "${title}"`));
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function orphansCommand(options: IndexOptions): Promise<void> {
  try {
    await withContext(async (ctx) => {
      // Check if index is built
      if (!isIndexBuilt(ctx.db)) {
        printWarning('Index has not been built yet');
        console.log();
        printInfo('Run "wikitool index rebuild" to build the index');
        return;
      }

      const orphans = getOrphanPages(ctx.db);
      const limit = parseInt(options.limit || '50', 10);
      const limited = orphans.slice(0, limit);

      console.log(chalk.bold('Orphan Pages'));
      console.log(chalk.dim('Pages with no incoming links'));
      console.log();

      if (limited.length === 0) {
        printSuccess('No orphan pages found');
        return;
      }

      for (const orphan of limited) {
        console.log(`  ${orphan.title}`);
      }

      console.log();
      if (orphans.length > limit) {
        printInfo(`Showing ${limit} of ${orphans.length} orphans (use --limit to show more)`);
      } else {
        console.log(chalk.dim(`${orphans.length} orphan page(s) found`));
      }
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
