/**
 * lint command - Lua linting for modules
 */

import chalk from 'chalk';
import ora from 'ora';
import { lintLuaContent, lintAllModules, isSeleneAvailable, type LuaLintResult } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printError, printInfo, printSection, printSuccess, printWarning } from '../utils/format.js';
import { buildMeta, withMeta } from '../utils/meta.js';

export interface LintOptions {
  format?: string;
  strict?: boolean;
  meta?: boolean;
}

export async function lintCommand(title?: string, options: LintOptions = {}): Promise<void> {
  if (!isSeleneAvailable()) {
    printWarning('Selene not found');
    printInfo('Run scripts/setup-selene.ps1 or scripts/setup-selene.sh, or set SELENE_PATH');
    return;
  }

  const spinner = ora('Linting Lua modules...').start();

  try {
    await withContext(async (ctx) => {
      let results: LuaLintResult[] = [];

      if (title) {
        const normalized = title.includes(':') ? title : `Module:${title}`;
        const page = ctx.db.getPage(normalized);
        if (!page) {
          spinner.stop();
          printError(`Module not found: ${normalized}`);
          process.exit(1);
        }
        const result = await lintLuaContent(page.content ?? '', normalized);
        results = [result];
      } else {
        results = await lintAllModules(ctx.db);
      }

      spinner.stop();

      const errors = results.flatMap(r => r.errors);
      const warnings = results.flatMap(r => r.warnings);
      const format = (options.format || 'text').toLowerCase();

      if (format === 'json') {
        const output = {
          results,
          summary: {
            errors: errors.length,
            warnings: warnings.length,
            total: errors.length + warnings.length,
          },
        };
        const data = options.meta === false ? output : withMeta(output, buildMeta(ctx));
        console.log(JSON.stringify(data, null, 2));
      } else {
        if (errors.length === 0 && warnings.length === 0) {
          printSuccess('No lint issues found');
        } else {
          for (const result of results) {
            if (result.errors.length === 0 && result.warnings.length === 0) continue;
            printSection(result.title);

            if (result.errors.length > 0) {
              for (const issue of result.errors) {
                printError(formatLintIssue(issue));
              }
            }

            if (result.warnings.length > 0) {
              for (const issue of result.warnings) {
                printWarning(formatLintIssue(issue));
              }
            }
          }

          console.log();
          console.log(chalk.bold(`Total: ${errors.length} error(s), ${warnings.length} warning(s)`));
        }
      }

      if (errors.length > 0 || (options.strict && warnings.length > 0)) {
        process.exit(1);
      }
    });
  } catch (error) {
    spinner.fail('Linting failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

function formatLintIssue(issue: { line: number; column: number; code: string; message: string }): string {
  return `${issue.line}:${issue.column} ${issue.code} ${issue.message}`;
}
