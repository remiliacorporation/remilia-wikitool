/**
 * seo command - SEO inspection helpers
 */

import chalk from 'chalk';
import ora from 'ora';
import { withContext } from '../utils/context.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import { printError, printSection, printSuccess, printWarning } from '../utils/format.js';
import { decodeHtml, extractHead, extractTitle, scanTags } from '../utils/html.js';
import { resolveTargetUrl } from '../utils/url.js';

export interface SeoInspectOptions {
  json?: boolean;
  meta?: boolean;
  url?: string;
}

export async function seoInspectCommand(target: string, options: SeoInspectOptions = {}): Promise<void> {
  const spinner = ora('Fetching page...').start();

  try {
    await withContext(async (ctx) => {
      const pageUrl = resolveTargetUrl(target, ctx.db, options.url);
      const response = await fetch(pageUrl, {
        headers: { 'User-Agent': 'Wikitool/1.0 (https://wiki.remilia.org)' },
      });

      if (!response.ok) {
        spinner.fail('Fetch failed');
        printError(`HTTP ${response.status}: ${response.statusText}`);
        process.exit(1);
      }

      const html = await response.text();
      const finalUrl = response.url;
      const head = extractHead(html);
      const titleTag = extractTitle(head);
      const metaTags = scanTags(head, 'meta');
      const linkTags = scanTags(head, 'link');

      const meta = collectMeta(metaTags);
      const canonical = findCanonical(linkTags);

      const result = {
        url: finalUrl,
        title: titleTag,
        meta: meta,
        canonical: canonical,
        missing: detectMissing(meta, titleTag, canonical),
      };

      spinner.stop();

      if (options.json) {
        const output = options.meta === false ? result : withMeta(result, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      printSection('SEO Inspect');
      console.log(`  URL: ${finalUrl}`);
      if (titleTag) {
        console.log(`  Title: ${titleTag}`);
      }
      if (canonical) {
        console.log(`  Canonical: ${canonical}`);
      }

      printSection('Meta');
      printMetaValue('description', meta['description']);
      printMetaValue('og:title', meta['og:title']);
      printMetaValue('og:description', meta['og:description']);
      printMetaValue('og:type', meta['og:type']);
      printMetaValue('og:image', meta['og:image']);
      printMetaValue('og:url', meta['og:url']);
      printMetaValue('twitter:card', meta['twitter:card']);
      printMetaValue('twitter:title', meta['twitter:title']);
      printMetaValue('twitter:description', meta['twitter:description']);
      printMetaValue('twitter:image', meta['twitter:image']);

      if (result.missing.length > 0) {
        printSection('Missing');
        for (const item of result.missing) {
          printWarning(item);
        }
      } else {
        printSuccess('No missing required tags');
      }
    });
  } catch (error) {
    spinner.fail('SEO inspection failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

function collectMeta(tags: Array<{ attrs: Record<string, string> }>): Record<string, string[]> {
  const meta: Record<string, string[]> = {};

  for (const tag of tags) {
    const attrs = tag.attrs;
    const key = (attrs.property || attrs.name || '').toLowerCase();
    if (!key) continue;
    const content = attrs.content ? decodeHtml(attrs.content).trim() : '';
    if (!content) continue;
    const list = meta[key] ?? [];
    list.push(content);
    meta[key] = list;
  }

  return meta;
}

function findCanonical(tags: Array<{ attrs: Record<string, string> }>): string | null {
  for (const tag of tags) {
    const rel = (tag.attrs.rel || '').toLowerCase();
    if (!rel.includes('canonical')) continue;
    const href = tag.attrs.href ? decodeHtml(tag.attrs.href).trim() : '';
    if (href) return href;
  }
  return null;
}

function detectMissing(meta: Record<string, string[]>, title: string | null, canonical: string | null): string[] {
  const missing: string[] = [];
  if (!title) missing.push('title tag');
  if (!meta['description']) missing.push('meta description');
  if (!meta['og:title']) missing.push('og:title');
  if (!meta['og:type']) missing.push('og:type');
  if (!meta['og:image']) missing.push('og:image');
  if (!meta['og:url']) missing.push('og:url');
  if (!canonical) missing.push('canonical link');
  if (!meta['twitter:card']) missing.push('twitter:card');
  return missing;
}

function printMetaValue(label: string, values?: string[]): void {
  if (!values || values.length === 0) {
    console.log(`  ${chalk.dim(label)}: ${chalk.dim('missing')}`);
    return;
  }
  const value = values[0];
  const extra = values.length > 1 ? chalk.dim(` (+${values.length - 1} more)`) : '';
  console.log(`  ${label}: ${value}${extra}`);
}
