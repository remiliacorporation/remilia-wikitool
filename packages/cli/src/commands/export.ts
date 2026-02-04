/**
 * export command - Export external wiki pages to AI-friendly markdown
 *
 * Fetches MediaWiki pages and converts to markdown with YAML frontmatter.
 * Supports exporting a page with all its subpages (as separate files).
 */

import chalk from 'chalk';
import ora from 'ora';
import * as fs from 'fs';
import * as path from 'path';
import {
  fetchPageByUrl,
  parseWikiUrl,
  wikitextToMarkdown,
  listSubpages,
  fetchPagesByTitles,
  type ExternalFetchResult,
} from '@wikitool/core';
import { printSuccess, printError, printInfo } from '../utils/format.js';
import { detectProjectContext } from '../utils/context.js';

/**
 * Feature flag: Use default exports directory when in wiki repo
 * Set WIKITOOL_NO_DEFAULT_EXPORTS=1 to disable
 */
const USE_DEFAULT_EXPORTS_DIR = !process.env.WIKITOOL_NO_DEFAULT_EXPORTS;
const DEFAULT_EXPORTS_DIR = 'wikitool_exports';

export interface ExportOptions {
  /** Output file/directory path */
  output?: string;
  /** Output format: markdown (default) or wikitext */
  format?: 'markdown' | 'wikitext';
  /** Hint for code block language detection */
  codeLanguage?: string;
  /** Skip YAML frontmatter */
  noFrontmatter?: boolean;
  /** Include all subpages */
  subpages?: boolean;
  /** Combine all subpages into single file (default: separate files) */
  combined?: boolean;
}

/**
 * Generate YAML frontmatter for exported content
 */
function generateFrontmatter(
  title: string,
  sourceUrl: string,
  domain: string,
  timestamp: string,
  extra?: Record<string, string | number>
): string {
  const lines = [
    '---',
    `title: "${title.replace(/"/g, '\\"')}"`,
    `source: ${sourceUrl}`,
    `wiki: ${domain}`,
    `fetched: ${timestamp}`,
  ];
  if (extra) {
    for (const [key, value] of Object.entries(extra)) {
      lines.push(`${key}: ${value}`);
    }
  }
  lines.push('---', '');
  return lines.join('\n');
}

/**
 * Convert a single page result to output format
 */
function convertPage(
  result: ExternalFetchResult,
  outputFormat: 'markdown' | 'wikitext',
  codeLanguage?: string
): string {
  if (outputFormat === 'wikitext') {
    return result.content;
  }
  // If content is already markdown, return as-is
  if (result.contentFormat === 'markdown') {
    return result.content;
  }
  return wikitextToMarkdown(result.content, {
    codeLanguage,
    preserveTemplateParams: true,
    stripRefs: false,
  });
}

/**
 * Sanitize title for use as filename
 */
function titleToFilename(title: string): string {
  return title
    .replace(/\//g, '_')  // Replace / with _
    .replace(/[<>:"|?*\\]/g, '-')  // Replace invalid chars
    .replace(/\s+/g, '-')  // Replace spaces with -
    .replace(/-+/g, '-')  // Collapse multiple dashes
    .replace(/^-|-$/g, '');  // Trim dashes
}

/**
 * Get default output path for exports
 * Returns path in wikitool_exports/ directory at repo root when in wiki repo
 */
function getDefaultOutputPath(title: string, isDirectory: boolean, format: 'markdown' | 'wikitext'): string | undefined {
  if (!USE_DEFAULT_EXPORTS_DIR) {
    return undefined;
  }

  const { projectRoot } = detectProjectContext();
  const exportsDir = path.join(projectRoot, DEFAULT_EXPORTS_DIR);
  const filename = titleToFilename(title);

  if (isDirectory) {
    return path.join(exportsDir, filename);
  }

  const ext = format === 'markdown' ? '.md' : '.wiki';
  return path.join(exportsDir, filename + ext);
}

/**
 * Export a single page
 */
async function exportSinglePage(
  url: string,
  options: ExportOptions
): Promise<void> {
  const parsed = parseWikiUrl(url);

  if (parsed) {
    console.log(chalk.bold(`Exporting: ${parsed.title}`));
    console.log(chalk.dim(`Source: ${parsed.domain}`));
  } else {
    console.log(chalk.bold(`Exporting: ${url}`));
    console.log(chalk.dim('Source: web (non-MediaWiki URL)'));
  }

  const outputFormat = options.format || 'markdown';
  console.log(chalk.dim(`Format: ${outputFormat}`));

  const spinner = ora('Fetching content...').start();

  const result = await fetchPageByUrl(url, { format: 'wikitext' });

  if (!result) {
    spinner.fail('Page not found');
    process.exit(1);
  }

  spinner.text = 'Converting content...';

  let output: string;
  const timestamp = new Date().toISOString();
  const converted = convertPage(result, outputFormat, options.codeLanguage);

  if (options.noFrontmatter) {
    output = converted;
  } else {
    const frontmatter = generateFrontmatter(
      result.title,
      result.url,
      result.sourceDomain || parsed?.domain || 'unknown',
      timestamp
    );
    output = frontmatter + '\n' + converted;
  }

  spinner.succeed('Content exported');

  // Use default output path if not specified
  const outputPath = options.output || getDefaultOutputPath(result.title, false, outputFormat);
  outputResult(output, outputPath, result.title, result.content.length, outputFormat);
}

/**
 * Export a page with all its subpages to separate files
 */
async function exportWithSubpages(
  url: string,
  options: ExportOptions
): Promise<void> {
  const parsed = parseWikiUrl(url);

  if (!parsed) {
    printError('Cannot determine wiki structure for subpage listing');
    printInfo('Subpages feature requires a recognized MediaWiki URL');
    process.exit(1);
  }

  // Remove trailing slash from title if present
  const mainTitle = parsed.title.replace(/\/$/, '');
  const domain = parsed.domain;

  console.log(chalk.bold(`Exporting: ${mainTitle} + subpages`));
  console.log(chalk.dim(`Source: ${domain}`));

  const outputFormat = options.format || 'markdown';
  const fileExt = outputFormat === 'markdown' ? '.md' : '.wiki';
  console.log(chalk.dim(`Format: ${outputFormat}`));

  const spinner = ora('Finding subpages...').start();

  // List all subpages
  const subpageTitles = await listSubpages(mainTitle, domain);
  spinner.text = `Found ${subpageTitles.length} subpages. Fetching main page...`;

  // Fetch main page first
  const mainResult = await fetchPageByUrl(url.replace(/\/$/, ''), { format: 'wikitext' });

  if (!mainResult) {
    spinner.fail('Main page not found');
    process.exit(1);
  }

  // Fetch all subpages
  spinner.text = `Fetching ${subpageTitles.length} subpages...`;
  const subpageResults = await fetchPagesByTitles(subpageTitles, domain);

  spinner.text = 'Converting content...';

  const timestamp = new Date().toISOString();
  const allResults = [mainResult, ...subpageResults];
  const totalOriginalLength = allResults.reduce((sum, r) => sum + r.content.length, 0);

  // Determine output path - use default exports dir if not specified
  const effectiveOutput = options.output || getDefaultOutputPath(mainTitle, true, outputFormat);

  // If --combined flag, or no output at all (including no default), use single file output
  if (options.combined || !effectiveOutput) {
    const sections: string[] = [];

    for (const result of allResults) {
      const converted = convertPage(result, outputFormat, options.codeLanguage);
      const sectionHeader = outputFormat === 'markdown'
        ? `# ${result.title}\n\n`
        : `== ${result.title} ==\n\n`;
      sections.push(sectionHeader + converted);
    }

    let output: string;
    const combinedContent = sections.join('\n\n---\n\n');

    if (options.noFrontmatter) {
      output = combinedContent;
    } else {
      const frontmatter = generateFrontmatter(
        mainTitle,
        mainResult.url,
        domain,
        timestamp,
        { subpages: subpageResults.length }
      );
      output = frontmatter + '\n' + combinedContent;
    }

    spinner.succeed(`Exported ${allResults.length} pages (1 main + ${subpageResults.length} subpages)`);
    // For combined output, use single file path (not directory)
    const combinedOutputPath = options.output || getDefaultOutputPath(mainTitle, false, outputFormat);
    outputResult(output, combinedOutputPath, mainTitle, totalOriginalLength, outputFormat);
    return;
  }

  // Separate files mode: output to directory
  const outputDir = effectiveOutput;
  fs.mkdirSync(outputDir, { recursive: true });

  const exportedFiles: Array<{ title: string; file: string; size: number }> = [];

  for (const result of allResults) {
    const converted = convertPage(result, outputFormat, options.codeLanguage);

    let output: string;
    if (options.noFrontmatter) {
      output = converted;
    } else {
      const frontmatter = generateFrontmatter(
        result.title,
        result.url,
        domain,
        timestamp
      );
      output = frontmatter + '\n' + converted;
    }

    const filename = titleToFilename(result.title) + fileExt;
    const filepath = path.join(outputDir, filename);
    fs.writeFileSync(filepath, output, 'utf-8');

    exportedFiles.push({
      title: result.title,
      file: filename,
      size: output.length,
    });
  }

  // Create index file
  const indexLines = [
    '---',
    `title: "${mainTitle} - Index"`,
    `source: ${mainResult.url}`,
    `wiki: ${domain}`,
    `fetched: ${timestamp}`,
    `pages: ${allResults.length}`,
    '---',
    '',
    `# ${mainTitle}`,
    '',
    `Exported ${allResults.length} pages from [${domain}](https://${domain}/${mainTitle.replace(/ /g, '_')}).`,
    '',
    '## Pages',
    '',
  ];

  for (const { title, file, size } of exportedFiles) {
    const sizeKb = (size / 1024).toFixed(1);
    indexLines.push(`- [${title}](./${file}) (${sizeKb} KB)`);
  }

  const indexContent = indexLines.join('\n');
  const indexPath = path.join(outputDir, '_index.md');
  fs.writeFileSync(indexPath, indexContent, 'utf-8');

  spinner.succeed(`Exported ${allResults.length} pages to ${outputDir}/`);

  // Show summary
  console.log();
  printSuccess(`Created ${allResults.length} files + index`);
  const totalSize = exportedFiles.reduce((sum, f) => sum + f.size, 0);
  console.log(chalk.dim(`Total size: ${(totalSize / 1024).toFixed(1)} KB`));
  console.log(chalk.dim(`Original: ${totalOriginalLength} characters`));
  console.log(chalk.dim(`Index: ${indexPath}`));
}

/**
 * Output the result to file or stdout
 */
function outputResult(
  output: string,
  outputPath: string | undefined,
  title: string,
  originalLength: number,
  outputFormat: string
): void {
  if (outputPath) {
    // Ensure output directory exists
    const outputDir = path.dirname(outputPath);
    if (outputDir && outputDir !== '.') {
      fs.mkdirSync(outputDir, { recursive: true });
    }

    fs.writeFileSync(outputPath, output, 'utf-8');
    printSuccess(`Saved to ${outputPath}`);

    // Show file stats
    const stats = fs.statSync(outputPath);
    console.log(chalk.dim(`Size: ${(stats.size / 1024).toFixed(1)} KB`));
  } else {
    // Output to stdout
    console.log();
    console.log(chalk.dim('-'.repeat(60)));
    console.log(output);
    console.log(chalk.dim('-'.repeat(60)));
  }

  // Show metadata
  console.log();
  console.log(chalk.dim(`Title: ${title}`));
  console.log(chalk.dim(`Original length: ${originalLength} characters`));
  if (outputFormat === 'markdown') {
    console.log(chalk.dim(`Converted length: ${output.length} characters`));
  }
}

export async function exportCommand(url: string, options: ExportOptions): Promise<void> {
  try {
    if (options.subpages) {
      await exportWithSubpages(url, options);
    } else {
      await exportSinglePage(url, options);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    printError(message);

    // Show supported formats if it's an unsupported URL
    if (message.includes('Unsupported') || message.includes('No working API')) {
      printInfo('Supported wiki URL formats:');
      console.log('  https://en.wikipedia.org/wiki/Page_name');
      console.log('  https://www.mediawiki.org/wiki/Page_name');
      console.log('  https://wowdev.wiki/Page_name');
      console.log('  https://any-domain.com/wiki/Page_name');
      console.log('  https://any-domain.com/Page_name (short URL MediaWiki)');
    }

    process.exit(1);
  }
}
