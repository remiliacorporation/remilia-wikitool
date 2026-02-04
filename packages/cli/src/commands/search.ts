/**
 * search command - Search local content using FTS
 */

import chalk from 'chalk';
import { withContext } from '../utils/context.js';
import { printInfo } from '../utils/format.js';

export interface SearchOptions {
  tier?: 'content' | 'extension' | 'technical';
  limit?: string;
}

export async function searchCommand(query: string, options: SearchOptions): Promise<void> {
  const limit = parseInt(options.limit || '20', 10);

  try {
    await withContext(async (ctx) => {
      console.log(chalk.bold(`Searching: ${query}`));
      if (options.tier) {
        console.log(chalk.dim(`Tier: ${options.tier}`));
      }
      console.log();

      const results = ctx.db.searchFts(query, {
        tier: options.tier,
        limit,
      });

      if (results.length === 0) {
        printInfo('No results found');

        // Suggest alternative
        if (!options.tier) {
          console.log(chalk.dim('Try specifying a tier: --tier content|extension|technical'));
        }
        return;
      }

      // Group by tier
      const byTier: Record<string, typeof results> = {};
      for (const result of results) {
        if (!byTier[result.tier]) {
          byTier[result.tier] = [];
        }
        byTier[result.tier].push(result);
      }

      // Display results
      for (const [tier, tierResults] of Object.entries(byTier)) {
        const tierColor = tier === 'content' ? chalk.green :
                         tier === 'extension' ? chalk.blue :
                         chalk.yellow;

        console.log(tierColor(`[${tier}]`));

        for (const result of tierResults) {
          console.log(`  ${chalk.bold(result.title)}`);
          // Clean up snippet (remove extra whitespace, HTML tags)
          const cleanSnippet = result.snippet
            .replace(/<\/?mark>/g, match => match === '<mark>' ? chalk.yellow.bold('') : '')
            .replace(/\s+/g, ' ')
            .trim();
          console.log(chalk.dim(`    ${cleanSnippet}`));
        }
        console.log();
      }

      console.log(chalk.dim(`Found ${results.length} results`));
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);

    // Handle FTS query errors gracefully
    if (message.includes('fts5')) {
      console.error(chalk.red('Search error:'), 'Invalid search query');
      console.log(chalk.dim('Tips:'));
      console.log(chalk.dim('  - Use quotes for phrases: "exact phrase"'));
      console.log(chalk.dim('  - Use OR for alternatives: word1 OR word2'));
      console.log(chalk.dim('  - Use - to exclude: word1 -word2'));
      process.exit(1);
    }

    console.error(chalk.red('Error:'), message);
    process.exit(1);
  }
}
