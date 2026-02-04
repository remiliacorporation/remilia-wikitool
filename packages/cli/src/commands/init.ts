/**
 * init command - Initialize wikitool database and configuration
 */

import chalk from 'chalk';
import ora from 'ora';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { withContext, detectProjectContext, type ProjectContext } from '../utils/context.js';
import { printSuccess, printError, printInfo, printSection } from '../utils/format.js';

export interface InitOptions {
  fromFiles?: boolean;
  templates?: boolean;
}

export async function initCommand(options: InitOptions): Promise<void> {
  const projectContext = detectProjectContext();

  console.log(chalk.bold('Initializing wikitool'));
  printInfo(`Mode: ${projectContext.mode}`);
  printInfo(`Project root: ${projectContext.projectRoot}`);
  printInfo(`Wikitool root: ${projectContext.wikitoolRoot}`);
  printInfo(`Content dir: ${projectContext.contentDir}`);
  printInfo(`Templates dir: ${projectContext.templatesDir}`);
  printInfo(`Database: ${projectContext.dbPath}`);

  if (projectContext.mode === 'standalone') {
    await ensureSiblingDirs(projectContext);
    await createEnvTemplate(projectContext);
  }

  const spinner = ora('Setting up database...').start();

  try {
    await withContext(async (ctx) => {
      spinner.succeed('Database initialized');

      // Ensure folder structure
      const folderSpinner = ora('Creating folder structure...').start();
      ctx.fs.ensureContentFolders();
      ctx.fs.ensureTemplateFolders();
      folderSpinner.succeed('Folder structure ready');

      // Initialize from existing files if requested
      if (options.fromFiles) {
        const fileSpinner = ora('Scanning existing files...').start();

        const result = await ctx.engine.initFromFiles({
          includeTemplates: options.templates,
        });

        if (result.errors.length > 0) {
          fileSpinner.warn(`Indexed ${result.added} files with ${result.errors.length} errors`);
          for (const error of result.errors.slice(0, 5)) {
            printError(error);
          }
          if (result.errors.length > 5) {
            printInfo(`... and ${result.errors.length - 5} more errors`);
          }
        } else {
          fileSpinner.succeed(`Indexed ${result.added} files`);
        }
      }

      // Show stats
      const stats = ctx.db.getStats();
      printSection('Database Statistics');
      console.log(`  Pages: ${stats.totalPages}`);
      console.log(`  Categories: ${stats.totalCategories}`);

      if (stats.totalPages > 0) {
        console.log('  By type:');
        for (const [type, count] of Object.entries(stats.byType)) {
          if (count > 0) {
            console.log(`    ${type}: ${count}`);
          }
        }
      }

      printSuccess('Initialization complete');

      if (!options.fromFiles && stats.totalPages === 0) {
        showNextSteps(ctx.projectContext ?? projectContext);
      }
    });
  } catch (error) {
    spinner.fail('Initialization failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

async function ensureSiblingDirs(ctx: ProjectContext): Promise<void> {
  const dirs = [
    ctx.contentDir,
    join(ctx.contentDir, 'Main'),
    join(ctx.contentDir, 'Category'),
    ctx.templatesDir,
  ];

  for (const dir of dirs) {
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
      printSuccess(`Created: ${relative(ctx.projectRoot, dir)}/`);
    }
  }
}

async function createEnvTemplate(ctx: ProjectContext): Promise<void> {
  const envTemplatePath = join(ctx.wikitoolRoot, '.env.template');
  if (!existsSync(envTemplatePath)) {
    const template = `# RemiliaWiki Bot Credentials
# Get these from Special:BotPasswords on wiki.remilia.org
WIKI_BOT_USER=YourUsername@BotName
WIKI_BOT_PASS=your-bot-password

# Wiki API URL (optional, defaults to wiki.remilia.org)
# WIKI_API_URL=https://wiki.remilia.org/api.php

# Rate limits in milliseconds (optional)
# WIKI_RATE_LIMIT_READ=300
# WIKI_RATE_LIMIT_WRITE=1000

# HTTP settings (optional)
# WIKI_HTTP_TIMEOUT_MS=30000
# WIKI_HTTP_RETRIES=2
`;
    writeFileSync(envTemplatePath, template);
    printSuccess('Created: .env.template');
    printInfo('Copy to ../.env and fill in your bot credentials');
  }
}

function showNextSteps(ctx: ProjectContext): void {
  console.log();
  printSection('Next steps');

  if (ctx.mode === 'standalone') {
    console.log('  1. Copy wikitool/.env.template to ../.env and add your bot credentials');
    console.log('     Get credentials from: https://wiki.remilia.org/wiki/Special:BotPasswords');
    console.log('  2. Pull content: bun run wikitool pull --full --all');
    console.log('  3. Edit files in wiki_content/ and templates/');
    console.log('  4. Review changes: bun run wikitool diff');
    console.log('  5. Push changes: bun run wikitool push --dry-run -s "Summary"');
  } else {
    console.log('  1. Pull content: bun run wikitool pull');
    console.log('  2. See CLAUDE.md for full workflow');
  }
}

