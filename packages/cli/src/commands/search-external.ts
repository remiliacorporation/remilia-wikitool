/**
 * search-external command - Search external wikis
 *
 * Search for pages on Wikipedia, MediaWiki.org, or custom MediaWiki domains.
 */

import chalk from 'chalk';
import ora from 'ora';
import { searchWiki, searchWikiByDomain, type WikiId } from '@wikitool/core';
import { printError } from '../utils/format.js';

export interface SearchExternalOptions {
  wiki?: string;
  lang?: string;
  domain?: string;
  apiUrl?: string;
  limit?: string;
}

export async function searchExternalCommand(
  query: string,
  options: SearchExternalOptions
): Promise<void> {
  const limit = parseInt(options.limit || '10', 10);

  // Determine source description
  let sourceDesc: string;
  if (options.domain) {
    sourceDesc = options.domain;
  } else if (options.wiki === 'mediawiki') {
    sourceDesc = 'MediaWiki.org';
  } else if (options.wiki === 'commons') {
    sourceDesc = 'Wikimedia Commons';
  } else if (options.wiki === 'wikidata') {
    sourceDesc = 'Wikidata';
  } else {
    sourceDesc = `${options.lang || 'en'}.wikipedia.org`;
  }

  console.log(chalk.bold(`Searching: "${query}"`));
  console.log(chalk.dim(`Source: ${sourceDesc}`));
  console.log();

  const spinner = ora('Searching...').start();

  try {
    let results;

    if (options.domain) {
      // Custom domain search
      const apiUrl = options.apiUrl
        || (options.domain.endsWith('.fandom.com')
          ? `https://${options.domain}/api.php`
          : undefined);
      results = await searchWikiByDomain(options.domain, query, { limit, apiUrl });
    } else {
      // Known wiki search
      const wiki = (options.wiki as WikiId) || 'wikipedia';
      results = await searchWiki(query, wiki, { lang: options.lang, limit });
    }

    spinner.stop();

    if (results.length === 0) {
      console.log(chalk.yellow('No results found.'));
      return;
    }

    console.log(chalk.dim(`Found ${results.length} result${results.length === 1 ? '' : 's'}:`));
    console.log();

    for (let i = 0; i < results.length; i++) {
      const result = results[i];
      // Title
      console.log(`${chalk.cyan(`${i + 1}.`)} ${chalk.bold(result.title)}`);

      // Snippet (cleaned)
      if (result.snippet) {
        const snippet = result.snippet.slice(0, 150) + (result.snippet.length > 150 ? '...' : '');
        console.log(`   ${chalk.dim(snippet)}`);
      }

      // Word count if available
      if (result.wordcount) {
        console.log(`   ${chalk.dim(`${result.wordcount} words`)}`);
      }

      // URL
      console.log(`   ${chalk.blue(result.url)}`);
      console.log();
    }
  } catch (error) {
    spinner.fail('Search failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
