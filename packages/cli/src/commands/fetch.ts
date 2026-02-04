/**
 * fetch command - Fetch content from external wikis
 *
 * Thin wrapper around @wikitool/core external client.
 */

import chalk from 'chalk';
import ora from 'ora';
import { fetchPageByUrl, parseWikiUrl } from '@wikitool/core';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printInfo } from '../utils/format.js';

export interface FetchOptions {
  format?: 'wikitext' | 'html';
  save?: boolean;
  name?: string;
}

export async function fetchCommand(url: string, options: FetchOptions): Promise<void> {
  // Parse the URL to show info
  const parsed = parseWikiUrl(url);

  if (parsed) {
    console.log(chalk.bold(`Fetching: ${parsed.title}`));
    console.log(chalk.dim(`Source: ${parsed.domain}`));
  } else {
    console.log(chalk.bold(`Fetching: ${url}`));
    console.log(chalk.dim('Source: web (non-MediaWiki URL)'));
  }

  const spinner = ora('Fetching content...').start();

  try {
    const result = await fetchPageByUrl(url, {
      format: options.format as 'wikitext' | 'html',
    });

    if (!result) {
      spinner.fail('Page not found');
      process.exit(1);
    }

    spinner.succeed('Content fetched');

    // Save to reference library if requested
    if (options.save) {
      await withContext(async (ctx) => {
        const name = options.name || result.title.replace(/[/\\:*?"<>|]/g, '_');
        const wikiType = result.sourceWiki || 'web';
        const filepath = `reference/${wikiType}/${name}.wiki`;

        // Write to file
        ctx.fs.writeFile(filepath, result.content);

        printSuccess(`Saved to reference library as "${name}"`);
        console.log(`  File: ${filepath}`);
      });
    } else {
      // Just output the content
      console.log();
      console.log(chalk.dim('─'.repeat(60)));
      console.log(result.content);
      console.log(chalk.dim('─'.repeat(60)));
    }

    // Show metadata
    console.log();
    console.log(chalk.dim(`Format: ${result.contentFormat || 'wikitext'}`));
    console.log(chalk.dim(`Length: ${result.content.length} characters`));
    if (result.sourceDomain) {
      console.log(chalk.dim(`Domain: ${result.sourceDomain}`));
    }
  } catch (error) {
    spinner.fail('Fetch failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);

    // Show supported formats if it's an unsupported URL
    if (message.includes('Unsupported') || message.includes('No working API')) {
      printInfo('Supported wiki URL formats:');
      console.log('  https://en.wikipedia.org/wiki/Page_name');
      console.log('  https://www.mediawiki.org/wiki/Page_name');
      console.log('  https://commons.wikimedia.org/wiki/Page_name');
      console.log('  https://wiki.miraheze.org/wiki/Page_name');
      console.log('  https://wiki.fandom.com/wiki/Page_name');
      console.log('  https://any-domain.com/wiki/Page_name (custom MediaWiki)');
      console.log('  https://example.com/page (generic web fetch)');
    }

    process.exit(1);
  }
}
