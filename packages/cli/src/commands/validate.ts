/**
 * validate command - Wiki content validation
 *
 * Checks for common issues:
 * - Broken internal links
 * - Orphan pages (no incoming links)
 * - Double redirects
 * - Uncategorized articles
 * - Missing SHORTDESC template
 */

import chalk from 'chalk';
import Table from 'cli-table3';
import ora from 'ora';
import { writeFileSync } from 'node:fs';
import { withContext } from '../utils/context.js';
import { printSuccess, printError, printSection, printInfo, printWarning } from '../utils/format.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import {
  isIndexBuilt,
  getBrokenLinks,
  getOrphanPages,
  getDoubleRedirects,
  getUncategorizedPages,
  getMissingShortdesc,
  type BrokenLinkResult,
  type DoubleRedirectResult,
  type OrphanResult,
} from '@wikitool/core';

export interface ValidateOptions {
  fix?: boolean;
  report?: string;
  format?: string;
  includeRemote?: boolean;
  remoteLimit?: string;
  limit?: string;
  meta?: boolean;
}

interface SpecialPageEntry {
  key: string;
  title: string;
  label: string;
  description: string;
  queryPage?: string;
}

interface SpecialPageSummary {
  key: string;
  title: string;
  label: string;
  description: string;
  url: string;
  fetchedAt: string | null;
  itemCount: number;
  items: string[];
  truncated: boolean;
  deletedLocalItems?: string[];
  error?: string;
}

interface ValidationReport {
  timestamp: string;
  issues: {
    brokenLinks: BrokenLinkResult[];
    orphanPages: OrphanResult[];
    doubleRedirects: DoubleRedirectResult[];
    uncategorizedPages: string[];
    missingShortdesc: string[];
    deletedLocal: string[];
  };
  summary: {
    brokenLinks: number;
    orphanPages: number;
    doubleRedirects: number;
    uncategorizedPages: number;
    missingShortdesc: number;
    deletedLocal: number;
    total: number;
  };
  specialPages: SpecialPageSummary[];
}

const SPECIAL_PAGES: SpecialPageEntry[] = [
  {
    key: 'wantedPages',
    title: 'Special:WantedPages',
    label: 'Wanted pages',
    description: 'Pages linked but not yet created.',
    queryPage: 'Wantedpages',
  },
  {
    key: 'shortPages',
    title: 'Special:ShortPages',
    label: 'Short pages',
    description: 'Very short pages that may need expansion.',
    queryPage: 'Shortpages',
  },
  {
    key: 'deadendPages',
    title: 'Special:DeadendPages',
    label: 'Dead-end pages',
    description: 'Pages with no outgoing links.',
    queryPage: 'Deadendpages',
  },
  {
    key: 'lonelyPages',
    title: 'Special:LonelyPages',
    label: 'Lonely pages',
    description: 'Pages with no incoming links.',
    queryPage: 'Lonelypages',
  },
  {
    key: 'uncategorizedPages',
    title: 'Special:UncategorizedPages',
    label: 'Uncategorized pages',
    description: 'Articles without categories.',
    queryPage: 'Uncategorizedpages',
  },
  {
    key: 'uncategorizedCategories',
    title: 'Special:UncategorizedCategories',
    label: 'Uncategorized categories',
    description: 'Categories without categories.',
    queryPage: 'Uncategorizedcategories',
  },
  {
    key: 'uncategorizedTemplates',
    title: 'Special:UncategorizedTemplates',
    label: 'Uncategorized templates',
    description: 'Templates without categories.',
    queryPage: 'Uncategorizedtemplates',
  },
  {
    key: 'wantedCategories',
    title: 'Special:WantedCategories',
    label: 'Wanted categories',
    description: 'Categories linked but not yet created.',
    queryPage: 'Wantedcategories',
  },
  {
    key: 'wantedTemplates',
    title: 'Special:WantedTemplates',
    label: 'Wanted templates',
    description: 'Templates linked but not yet created.',
    queryPage: 'Wantedtemplates',
  },
  {
    key: 'unusedTemplates',
    title: 'Special:UnusedTemplates',
    label: 'Unused templates',
    description: 'Templates with no transclusions.',
    queryPage: 'Unusedtemplates',
  },
  {
    key: 'brokenRedirects',
    title: 'Special:BrokenRedirects',
    label: 'Broken redirects',
    description: 'Redirects pointing to missing targets.',
    queryPage: 'BrokenRedirects',
  },
];

function deriveWikiUrl(apiUrl: string | null): string | null {
  if (!apiUrl) return null;
  try {
    const url = new URL(apiUrl);
    const pathLower = url.pathname.toLowerCase();
    if (pathLower.endsWith('/api.php')) {
      url.pathname = url.pathname.slice(0, -8);
    } else if (pathLower.endsWith('api.php')) {
      url.pathname = url.pathname.slice(0, -7);
    }
    const normalized = trimTrailingSlash(url.toString());
    return normalized.length > 0 ? normalized : null;
  } catch {
    return null;
  }
}

function resolveWikiUrl(db: { getConfig: (key: string) => string | null }): string {
  const envUrl = process.env.WIKI_URL;
  if (envUrl) return trimTrailingSlash(envUrl);
  const configUrl = db.getConfig('wiki_url');
  if (configUrl) return trimTrailingSlash(configUrl);
  const apiUrl = db.getConfig('wiki_api_url');
  const derived = deriveWikiUrl(apiUrl);
  return derived || 'https://wiki.remilia.org';
}

function buildSpecialPageUrl(baseUrl: string, title: string): string {
  const trimmedBase = trimTrailingSlash(baseUrl);
  const trimmedTitle = trimLeadingSlash(title);
  return `${trimmedBase}/${trimmedTitle}`;
}

function decodeHtml(text: string): string {
  let decoded = text;
  decoded = replaceAllLiteral(decoded, '&amp;', '&');
  decoded = replaceAllLiteral(decoded, '&quot;', '"');
  decoded = replaceAllLiteral(decoded, '&#39;', "'");
  decoded = replaceAllLiteral(decoded, '&lt;', '<');
  decoded = replaceAllLiteral(decoded, '&gt;', '>');
  return decoded;
}

function extractSpecialPageItems(html: string): string[] {
  const items: string[] = [];
  const seen = new Set<string>();

  let i = 0;
  while (i < html.length) {
    const liStart = indexOfIgnoreCase(html, '<li', i);
    if (liStart === -1) break;
    const liOpenEnd = findChar(html, '>', liStart + 3);
    if (liOpenEnd === null) break;
    const liClose = indexOfIgnoreCase(html, '</li>', liOpenEnd + 1);
    if (liClose === -1) break;

    const liContent = html.slice(liOpenEnd + 1, liClose);
    const anchor = extractFirstAnchor(liContent);
    if (anchor) {
      const rawTitle = anchor.title ? decodeHtml(anchor.title) : '';
      const rawText = anchor.text ? decodeHtml(anchor.text) : '';
      let title = (rawText || rawTitle).trim();

      if (title && !isNextPrevPage(title)) {
        title = stripSuffixIgnoreCase(title.trim(), ' (page does not exist)');
        if (title && !seen.has(title)) {
          seen.add(title);
          items.push(title);
        }
      }
    }

    i = liClose + 5;
  }

  return items;
}

function extractFirstAnchor(html: string): { title: string | null; text: string | null } | null {
  const aStart = indexOfIgnoreCase(html, '<a', 0);
  if (aStart === -1) return null;
  const aOpenEnd = findChar(html, '>', aStart + 2);
  if (aOpenEnd === null) return null;
  const aClose = indexOfIgnoreCase(html, '</a>', aOpenEnd + 1);
  if (aClose === -1) return null;

  const openTag = html.slice(aStart, aOpenEnd + 1);
  const title = readAttribute(openTag, 'title');
  const innerRaw = html.slice(aOpenEnd + 1, aClose);
  const text = stripTags(innerRaw).trim();

  return { title, text };
}

function readAttribute(tag: string, attr: string): string | null {
  let i = 0;
  const target = attr.toLowerCase();
  while (i < tag.length) {
    if (isWhitespace(tag[i])) {
      i++;
      continue;
    }
    const nameStart = i;
    while (i < tag.length && isAttrChar(tag[i])) i++;
    const name = tag.slice(nameStart, i).toLowerCase();
    if (name === target) {
      i = skipWhitespace(tag, i);
      if (tag[i] !== '=') return null;
      i = skipWhitespace(tag, i + 1);
      const quote = tag[i];
      if (quote !== '"' && quote !== '\'') return null;
      i++;
      const valueStart = i;
      while (i < tag.length && tag[i] !== quote) i++;
      return tag.slice(valueStart, i);
    }
    i++;
  }
  return null;
}

function stripTags(text: string): string {
  let out = '';
  let i = 0;
  while (i < text.length) {
    if (text[i] === '<') {
      const end = findChar(text, '>', i + 1);
      if (end === null) break;
      i = end + 1;
      continue;
    }
    out += text[i];
    i++;
  }
  return out;
}

function isNextPrevPage(text: string): boolean {
  const trimmed = text.trim().toLowerCase();
  if (trimmed.startsWith('next page')) return true;
  if (trimmed.startsWith('previous page')) return true;
  return false;
}

function stripSuffixIgnoreCase(text: string, suffix: string): string {
  if (text.length < suffix.length) return text;
  const start = text.length - suffix.length;
  for (let i = 0; i < suffix.length; i++) {
    if (text[start + i].toLowerCase() !== suffix[i].toLowerCase()) return text;
  }
  return text.slice(0, start).trim();
}

function trimTrailingSlash(text: string): string {
  let end = text.length;
  while (end > 0 && text[end - 1] === '/') end--;
  return text.slice(0, end);
}

function trimLeadingSlash(text: string): string {
  let start = 0;
  while (start < text.length && text[start] === '/') start++;
  return text.slice(start);
}

function replaceAllLiteral(text: string, search: string, replacement: string): string {
  if (!search) return text;
  let out = '';
  let i = 0;
  while (i < text.length) {
    if (text.startsWith(search, i)) {
      out += replacement;
      i += search.length;
    } else {
      out += text[i];
      i++;
    }
  }
  return out;
}

function getDeletedLocalFast(
  db: { getPages: (options?: { namespace?: number }) => Array<{ title: string; namespace: number; filepath: string }> },
  fs: { fileExists: (filepath: string) => boolean }
): string[] {
  const deleted: string[] = [];
  // Content namespaces only (main, category, file, user, goldenlight)
  const namespaces = new Set<number>([0, 14, 6, 2, 3000]);

  const pages = db.getPages();
  for (const page of pages) {
    if (!namespaces.has(page.namespace)) continue;
    if (!page.filepath) continue;
    if (!fs.fileExists(page.filepath)) {
      deleted.push(page.title);
    }
  }

  deleted.sort((a, b) => a.localeCompare(b));
  return deleted;
}

function findChar(text: string, target: string, start: number): number | null {
  for (let i = start; i < text.length; i++) {
    if (text[i] === target) return i;
  }
  return null;
}

function indexOfIgnoreCase(text: string, search: string, start: number): number {
  if (!search) return start;
  for (let i = start; i <= text.length - search.length; i++) {
    let match = true;
    for (let j = 0; j < search.length; j++) {
      if (text[i + j].toLowerCase() !== search[j].toLowerCase()) {
        match = false;
        break;
      }
    }
    if (match) return i;
  }
  return -1;
}

function isWhitespace(ch: string): boolean {
  return ch === ' ' || ch === '\\t' || ch === '\\n' || ch === '\\r';
}

function isAttrChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || ch === '-' || ch === '_';
}

function skipWhitespace(text: string, start: number): number {
  let i = start;
  while (i < text.length && isWhitespace(text[i])) i++;
  return i;
}

async function fetchSpecialPageSummary(
  client: {
    getParsedHtml: (title: string) => Promise<string | null>;
    getQueryPageItems: (page: string, options: { limit?: number }) => Promise<{ items: Array<{ title: string }>; truncated: boolean }>;
  },
  wikiUrl: string,
  page: SpecialPageEntry,
  limit: number
): Promise<SpecialPageSummary> {
  const url = buildSpecialPageUrl(wikiUrl, page.title);
  try {
    const fetchedAt = new Date().toISOString();
    if (page.queryPage) {
      try {
        const qp = await client.getQueryPageItems(page.queryPage, { limit });
        const items = qp.items.map(item => item.title).filter(title => title);
        return {
          ...page,
          url,
          fetchedAt,
          itemCount: items.length,
          items,
          truncated: qp.truncated,
        };
      } catch (error) {
        const fallback = await client.getParsedHtml(page.title);
        if (!fallback) {
          return {
            ...page,
            url,
            fetchedAt,
            itemCount: 0,
            items: [],
            truncated: false,
            error: error instanceof Error ? error.message : String(error),
          };
        }

        const items = extractSpecialPageItems(fallback);
        const truncated = limit > 0 && items.length > limit;

        return {
          ...page,
          url,
          fetchedAt,
          itemCount: items.length,
          items: limit > 0 ? items.slice(0, limit) : items,
          truncated,
        };
      }
    }

    const html = await client.getParsedHtml(page.title);
    if (!html) {
      return {
        ...page,
        url,
        fetchedAt,
        itemCount: 0,
        items: [],
        truncated: false,
        error: 'No HTML returned',
      };
    }

    const items = extractSpecialPageItems(html);
    const truncated = limit > 0 && items.length > limit;

    return {
      ...page,
      url,
      fetchedAt,
      itemCount: items.length,
      items: limit > 0 ? items.slice(0, limit) : items,
      truncated,
    };
  } catch (error) {
    return {
      ...page,
      url,
      fetchedAt: new Date().toISOString(),
      itemCount: 0,
      items: [],
      truncated: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

function renderMarkdownReport(report: ValidationReport): string {
  const lines: string[] = [];

  lines.push(`# Wikitool Validation Report`);
  lines.push('');
  lines.push(`Generated: ${report.timestamp}`);
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(`- Broken links: ${report.summary.brokenLinks}`);
  lines.push(`- Orphan pages: ${report.summary.orphanPages}`);
  lines.push(`- Double redirects: ${report.summary.doubleRedirects}`);
  lines.push(`- Uncategorized articles: ${report.summary.uncategorizedPages}`);
  lines.push(`- Missing SHORTDESC: ${report.summary.missingShortdesc}`);
  lines.push(`- Deleted locally (pending delete): ${report.summary.deletedLocal}`);
  lines.push(`- Total issues: ${report.summary.total}`);
  lines.push('');

  const section = (title: string, items: string[]) => {
    lines.push(`## ${title}`);
    lines.push('');
    if (items.length === 0) {
      lines.push('_None_');
      lines.push('');
      return;
    }
    for (const item of items) {
      lines.push(`- ${item}`);
    }
    lines.push('');
  };

  section('Broken Links', report.issues.brokenLinks.map(link => `${link.sourceTitle} -> ${link.targetTitle}`));
  section('Orphan Pages', report.issues.orphanPages.map(orphan => orphan.title));
  section('Double Redirects', report.issues.doubleRedirects.map(dr => `${dr.title} -> ${dr.finalTarget}`));
  section('Uncategorized Articles', report.issues.uncategorizedPages);
  section('Missing SHORTDESC', report.issues.missingShortdesc);
  section('Deleted locally (pending delete)', report.issues.deletedLocal);

  lines.push('## Special Pages (Remote)');
  lines.push('');
  for (const page of report.specialPages) {
    lines.push(`### ${page.label}`);
    lines.push('');
    lines.push(`- Title: ${page.title}`);
    lines.push(`- Description: ${page.description}`);
    lines.push(`- URL: ${page.url}`);
    lines.push(`- Fetched at: ${page.fetchedAt ?? 'not fetched'}`);
    lines.push(`- Items: ${page.itemCount}`);
    if (page.truncated) {
      lines.push(`- Note: list truncated`);
    }
    if (page.error) {
      lines.push(`- Error: ${page.error}`);
    }
    if (page.items.length > 0) {
      lines.push('');
      const deletedSet = new Set(page.deletedLocalItems ?? []);
      for (const item of page.items) {
        if (deletedSet.has(item)) {
          lines.push(`- ${item} (deleted locally; run wikitool push --delete)`);
        } else {
          lines.push(`- ${item}`);
        }
      }
    }
    lines.push('');
  }

  return lines.join('\n');
}

export async function validateCommand(options: ValidateOptions): Promise<void> {
  console.log(chalk.bold('Wiki Content Validation'));
  console.log();

  const spinner = ora('Running validation checks...').start();

  try {
    await withContext(async (ctx) => {
      // Check if index is built
      if (!isIndexBuilt(ctx.db)) {
        spinner.fail('Index not built');
        printWarning('Index has not been built yet');
        console.log();
        printInfo('Run "wikitool index rebuild" to build the index first');
        process.exit(1);
      }

      // Run all validation checks
      spinner.text = 'Checking for broken links...';
      const brokenLinks = getBrokenLinks(ctx.db);

      spinner.text = 'Finding orphan pages...';
      const orphanPages = getOrphanPages(ctx.db);

      spinner.text = 'Checking for double redirects...';
      const doubleRedirects = getDoubleRedirects(ctx.db);

      spinner.text = 'Finding uncategorized pages...';
      const uncategorizedPages = getUncategorizedPages(ctx.db);

      spinner.text = 'Checking for missing SHORTDESC...';
      const missingShortdesc = getMissingShortdesc(ctx.db);

      spinner.text = 'Checking for locally deleted pages...';
      const deletedLocal = getDeletedLocalFast(ctx.db, ctx.fs);

      const wikiUrl = resolveWikiUrl(ctx.db);
      const remoteLimitRaw = parseInt(options.remoteLimit || '200', 10);
      const remoteLimit = Number.isFinite(remoteLimitRaw) ? remoteLimitRaw : 200;

      let specialPages: SpecialPageSummary[] = [];
      if (options.includeRemote) {
        spinner.text = 'Fetching Special: pages...';
        specialPages = [];
        for (const page of SPECIAL_PAGES) {
          const summary = await fetchSpecialPageSummary(ctx.client, wikiUrl, page, remoteLimit);
          specialPages.push(summary);
        }
      } else {
        specialPages = SPECIAL_PAGES.map(page => ({
          ...page,
          url: buildSpecialPageUrl(wikiUrl, page.title),
          fetchedAt: null,
          itemCount: 0,
          items: [],
          truncated: false,
        }));
      }

      if (deletedLocal.length > 0) {
        const deletedSet = new Set(deletedLocal);
        specialPages = specialPages.map(page => {
          if (!page.items || page.items.length === 0) return page;
          const localDeleted = page.items.filter(item => deletedSet.has(item));
          if (localDeleted.length === 0) return page;
          return { ...page, deletedLocalItems: localDeleted };
        });
      }

      spinner.stop();

      // Summary table
      const summaryTable = new Table({
        head: [chalk.bold('Issue Type'), chalk.bold('Count')],
        style: { head: [], border: [] },
      });

      summaryTable.push(
        ['Broken links', brokenLinks.length.toString()],
        ['Orphan pages', orphanPages.length.toString()],
        ['Double redirects', doubleRedirects.length.toString()],
        ['Uncategorized articles', uncategorizedPages.length.toString()],
        ['Missing SHORTDESC', missingShortdesc.length.toString()],
        ['Deleted locally', deletedLocal.length.toString()],
      );

      console.log(summaryTable.toString());

      const totalIssues = brokenLinks.length + orphanPages.length +
        doubleRedirects.length + uncategorizedPages.length + missingShortdesc.length + deletedLocal.length;

      console.log();
      if (totalIssues === 0) {
        printSuccess('No issues found!');
        return;
      }

      const limit = parseInt(options.limit || '10', 10);

      // Show broken links
      if (brokenLinks.length > 0) {
        printSection('Broken Links');
        console.log(chalk.dim('Internal links to non-existent pages'));
        console.log();

        const linkTable = new Table({
          head: [chalk.bold('Source Page'), chalk.bold('Broken Link')],
          style: { head: [], border: [] },
        });

        for (const link of brokenLinks.slice(0, limit)) {
          linkTable.push([link.sourceTitle, chalk.red(link.targetTitle)]);
        }
        console.log(linkTable.toString());

        if (brokenLinks.length > limit) {
          printInfo(`... and ${brokenLinks.length - limit} more`);
        }
      }

      // Show orphan pages
      if (orphanPages.length > 0) {
        printSection('Orphan Pages');
        console.log(chalk.dim('Pages with no incoming links'));
        console.log();

        for (const orphan of orphanPages.slice(0, limit)) {
          console.log(`  ${chalk.yellow(orphan.title)}`);
        }

        if (orphanPages.length > limit) {
          console.log();
          printInfo(`... and ${orphanPages.length - limit} more`);
        }
      }

      // Show double redirects
      if (doubleRedirects.length > 0) {
        printSection('Double Redirects');
        console.log(chalk.dim('Redirects that point to other redirects'));
        console.log();

        const redirectTable = new Table({
          head: [chalk.bold('Redirect'), chalk.bold('First Target'), chalk.bold('Final Target')],
          style: { head: [], border: [] },
        });

        for (const dr of doubleRedirects.slice(0, limit)) {
          redirectTable.push([
            dr.title,
            chalk.yellow(dr.firstTarget),
            chalk.green(dr.finalTarget)
          ]);
        }
        console.log(redirectTable.toString());

        if (doubleRedirects.length > limit) {
          printInfo(`... and ${doubleRedirects.length - limit} more`);
        }
      }

      // Show uncategorized pages
      if (uncategorizedPages.length > 0) {
        printSection('Uncategorized Articles');
        console.log(chalk.dim('Articles with no category assignments'));
        console.log();

        for (const page of uncategorizedPages.slice(0, limit)) {
          console.log(`  ${chalk.yellow(page)}`);
        }

        if (uncategorizedPages.length > limit) {
          console.log();
          printInfo(`... and ${uncategorizedPages.length - limit} more`);
        }
      }

      // Show missing SHORTDESC
      if (missingShortdesc.length > 0) {
        printSection('Missing SHORTDESC');
        console.log(chalk.dim('Articles without {{SHORTDESC:...}} template'));
        console.log();

        for (const page of missingShortdesc.slice(0, limit)) {
          console.log(`  ${chalk.yellow(page)}`);
        }

        if (missingShortdesc.length > limit) {
          console.log();
          printInfo(`... and ${missingShortdesc.length - limit} more`);
        }
      }

      // Show deleted locally
      if (deletedLocal.length > 0) {
        printSection('Deleted Locally (Pending Delete)');
        console.log(chalk.dim('Pages removed locally but still present on the wiki'));
        console.log();

        for (const page of deletedLocal.slice(0, limit)) {
          console.log(`  ${chalk.magenta(page)}`);
        }

        if (deletedLocal.length > limit) {
          console.log();
          printInfo(`... and ${deletedLocal.length - limit} more`);
        }

        console.log();
        printInfo('Use "wikitool diff" then "wikitool push --dry-run --delete" before deleting on the wiki');
      }

      // Export report if requested
      if (options.report) {
        const report: ValidationReport = {
          timestamp: new Date().toISOString(),
          issues: {
            brokenLinks,
            orphanPages,
            doubleRedirects,
            uncategorizedPages,
            missingShortdesc,
            deletedLocal,
          },
          summary: {
            brokenLinks: brokenLinks.length,
            orphanPages: orphanPages.length,
            doubleRedirects: doubleRedirects.length,
            uncategorizedPages: uncategorizedPages.length,
            missingShortdesc: missingShortdesc.length,
            deletedLocal: deletedLocal.length,
            total: totalIssues,
          },
          specialPages,
        };

        const format = (options.format || 'json').toLowerCase();
        const reportOutput = format === 'md' || format === 'markdown'
          ? renderMarkdownReport(report)
          : JSON.stringify(
            options.meta === false ? report : withMeta(report, buildMeta(ctx)),
            null,
            2
          );

        writeFileSync(options.report, reportOutput);
        console.log();
        printSuccess(`Report saved to ${options.report}`);
      }

      // Auto-fix message
      if (options.fix) {
        console.log();
        printWarning('Auto-fix intentionally disabled');
        printInfo('Use the report output to review issues before making manual changes');
      }

      console.log();
      printWarning(`Found ${totalIssues} issue(s) that need attention`);
      printInfo('Use --limit N to show more results, or --report <file> to export');
    });
  } catch (error) {
    spinner.fail('Validation failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}
