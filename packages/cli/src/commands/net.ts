/**
 * net command - Network inspection helpers
 */

import chalk from 'chalk';
import ora from 'ora';
import { withContext } from '../utils/context.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import { formatBytes, printError, printSection, printSuccess, printWarning } from '../utils/format.js';
import { extractHead, scanTags } from '../utils/html.js';
import { resolveTargetUrl } from '../utils/url.js';

export interface NetInspectOptions {
  json?: boolean;
  meta?: boolean;
  limit?: string;
  probe?: boolean;
  url?: string;
}

interface ResourceEntry {
  url: string;
  type: string;
  tag: string;
  sizeBytes?: number;
  contentType?: string;
  cacheControl?: string;
  age?: string | null;
  xCache?: string | null;
  xVarnish?: string | null;
}

export async function netInspectCommand(target: string, options: NetInspectOptions = {}): Promise<void> {
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

      const resources = collectResources(head, html, finalUrl);
      const limit = parseLimit(options.limit, 25);
      const limited = resources.slice(0, limit);
      const shouldProbe = options.probe !== false;

      if (shouldProbe) {
        spinner.text = 'Probing resources...';
        await probeResourcesConcurrent(limited, 6);
      }

      spinner.stop();

      const summary = buildSummary(resources, limited);
      const result = {
        url: finalUrl,
        totalResources: resources.length,
        inspected: limited.length,
        summary,
        resources: limited,
      };

      if (options.json) {
        const output = options.meta === false ? result : withMeta(result, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      printSection('Network Inspect');
      console.log(`  URL: ${finalUrl}`);
      console.log(`  Resources found: ${resources.length}`);
      console.log(`  Inspected: ${limited.length}`);

      printSection('Totals');
      console.log(`  Known bytes: ${formatBytes(summary.knownBytes)}`);
      console.log(`  Unknown sizes: ${summary.unknownCount}`);

      if (summary.largest.length > 0) {
        printSection('Largest resources');
        for (const entry of summary.largest) {
          const sizeLabel = entry.sizeBytes ? formatBytes(entry.sizeBytes) : chalk.dim('unknown');
          console.log(`  ${sizeLabel} ${entry.type} ${entry.url}`);
        }
      }

      if (summary.cacheWarnings.length > 0) {
        printSection('Cache warnings');
        for (const warning of summary.cacheWarnings) {
          printWarning(warning);
        }
      } else {
        printSuccess('No cache warnings detected');
      }
    });
  } catch (error) {
    spinner.fail('Network inspection failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

function collectResources(head: string, html: string, baseUrl: string): ResourceEntry[] {
  const resources: ResourceEntry[] = [];
  const seen = new Set<string>();

  const scripts = scanTags(html, 'script');
  for (const tag of scripts) {
    const src = tag.attrs.src;
    if (!src) continue;
    pushResource(resources, seen, src, baseUrl, { type: 'script', tag: 'script' });
  }

  const links = scanTags(head, 'link');
  for (const tag of links) {
    const rel = (tag.attrs.rel || '').toLowerCase();
    const href = tag.attrs.href;
    if (!href) continue;
    if (rel.includes('stylesheet')) {
      pushResource(resources, seen, href, baseUrl, { type: 'style', tag: 'link' });
      continue;
    }
    if (rel.includes('preload') || rel.includes('modulepreload')) {
      const asType = (tag.attrs.as || '').toLowerCase();
      const type = asType || 'preload';
      pushResource(resources, seen, href, baseUrl, { type, tag: 'link' });
    }
  }

  const images = scanTags(html, 'img');
  for (const tag of images) {
    const src = tag.attrs.src || extractSrcset(tag.attrs.srcset);
    if (!src) continue;
    pushResource(resources, seen, src, baseUrl, { type: 'image', tag: 'img' });
  }

  const sources = scanTags(html, 'source');
  for (const tag of sources) {
    const src = tag.attrs.src || extractSrcset(tag.attrs.srcset);
    if (!src) continue;
    const type = tag.attrs.type ? tag.attrs.type.split('/')[0] : 'source';
    pushResource(resources, seen, src, baseUrl, { type, tag: 'source' });
  }

  return resources;
}

function pushResource(
  resources: ResourceEntry[],
  seen: Set<string>,
  rawUrl: string,
  baseUrl: string,
  meta: { type: string; tag: string }
): void {
  const normalized = rawUrl.trim();
  if (!normalized) return;
  if (startsWithIgnoreCase(normalized, 'data:') || startsWithIgnoreCase(normalized, 'javascript:')) return;

  let absolute: string;
  try {
    absolute = new URL(normalized, baseUrl).toString();
  } catch {
    return;
  }

  if (seen.has(absolute)) return;
  seen.add(absolute);
  resources.push({ url: absolute, type: meta.type, tag: meta.tag });
}

async function probeResource(entry: ResourceEntry): Promise<void> {
  try {
    const response = await fetch(entry.url, { method: 'HEAD' });
    if (!response.ok) {
      return;
    }
    const length = response.headers.get('content-length');
    if (length) {
      const parsed = Number(length);
      if (Number.isFinite(parsed)) {
        entry.sizeBytes = parsed;
      }
    }
    entry.contentType = response.headers.get('content-type') || undefined;
    entry.cacheControl = response.headers.get('cache-control') || undefined;
    entry.age = response.headers.get('age');
    entry.xCache = response.headers.get('x-cache');
    entry.xVarnish = response.headers.get('x-varnish');
  } catch {
    // ignore probe failures
  }
}

async function probeResourcesConcurrent(entries: ResourceEntry[], concurrency: number): Promise<void> {
  for (let i = 0; i < entries.length; i += concurrency) {
    const batch = entries.slice(i, i + concurrency);
    await Promise.all(batch.map(entry => probeResource(entry)));
  }
}

function buildSummary(all: ResourceEntry[], inspected: ResourceEntry[]) {
  let knownBytes = 0;
  let unknownCount = 0;
  const cacheWarnings: string[] = [];

  for (const entry of inspected) {
    if (entry.sizeBytes) {
      knownBytes += entry.sizeBytes;
    } else {
      unknownCount += 1;
    }

    if (entry.cacheControl && entry.cacheControl.includes('no-store')) {
      cacheWarnings.push(`no-store: ${entry.url}`);
    }
    if (!entry.cacheControl) {
      cacheWarnings.push(`missing cache-control: ${entry.url}`);
    }
  }

  const largest = inspected
    .filter(entry => entry.sizeBytes)
    .sort((a, b) => (b.sizeBytes ?? 0) - (a.sizeBytes ?? 0))
    .slice(0, 5);

  return { knownBytes, unknownCount, largest, cacheWarnings };
}

function parseLimit(raw?: string, fallback = 25): number {
  if (!raw) return fallback;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed <= 0) return fallback;
  return Math.floor(parsed);
}

function extractSrcset(srcset?: string): string | null {
  if (!srcset) return null;
  let i = 0;
  let current = '';
  while (i < srcset.length) {
    const ch = srcset[i];
    if (ch === ',') break;
    current += ch;
    i++;
  }
  const trimmed = current.trim();
  if (!trimmed) return null;
  const space = trimmed.indexOf(' ');
  if (space === -1) return trimmed;
  return trimmed.slice(0, space);
}

function startsWithIgnoreCase(text: string, prefix: string): boolean {
  if (text.length < prefix.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (text[i].toLowerCase() !== prefix[i]) return false;
  }
  return true;
}

