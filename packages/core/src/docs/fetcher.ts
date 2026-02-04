/**
 * Documentation fetcher for mediawiki.org
 *
 * Fetches extension documentation (Tier 2) and technical documentation (Tier 3)
 * from mediawiki.org and stores them in SQLite for FTS.
 */

import { createHash } from 'node:crypto';

/** MediaWiki.org API URL */
const MEDIAWIKI_API = 'https://www.mediawiki.org/w/api.php';

/** User agent for API requests */
const USER_AGENT = 'Wikitool/1.0 (https://wiki.remilia.org)';

/** Rate limit between requests (ms) */
const RATE_LIMIT_MS = 300;

/** Cache TTL in days */
export const DOCS_CACHE_TTL_DAYS = 7;

/** Sleep helper */
function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

/** Compute SHA-256 hash (first 16 chars) */
function computeHash(content: string): string {
  return createHash('sha256').update(content).digest('hex').slice(0, 16);
}

/** Extension doc metadata */
export interface ExtensionDocInfo {
  extensionName: string;
  sourceWiki: string;
  version: string | null;
  pagesCount: number;
  fetchedAt: string;
  expiresAt: string;
}

/** Extension doc page */
export interface ExtensionDocPage {
  extensionId?: number;
  pageTitle: string;
  localPath: string;
  content: string;
  contentHash: string;
  fetchedAt: string;
}

/** Technical doc record */
export interface TechnicalDoc {
  docType: 'hooks' | 'config' | 'api' | 'manual';
  pageTitle: string;
  localPath: string;
  content: string;
  contentHash: string;
  fetchedAt: string;
  expiresAt: string;
}

/** Fetch result */
export interface FetchResult {
  success: boolean;
  pagesImported: number;
  errors: string[];
}

/** Progress callback */
export type ProgressCallback = (current: number, total: number, message?: string) => void;

/**
 * Fetch a page from MediaWiki.org
 */
async function fetchMediaWikiPage(
  title: string
): Promise<{ content: string; timestamp: string } | null> {
  const params = new URLSearchParams({
    action: 'query',
    titles: title,
    prop: 'revisions',
    rvprop: 'content|timestamp',
    rvslots: 'main',
    format: 'json',
    formatversion: '2',
  });

  const response = await fetch(`${MEDIAWIKI_API}?${params.toString()}`, {
    headers: { 'User-Agent': USER_AGENT },
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }

  const data = await response.json() as {
    query?: {
      pages?: Array<{
        missing?: boolean;
        revisions?: Array<{
          slots?: { main?: { content?: string } };
          timestamp?: string;
        }>;
      }>;
    };
  };

  const page = data.query?.pages?.[0];
  if (!page || page.missing || !page.revisions?.[0]) {
    return null;
  }

  const revision = page.revisions[0];
  const content = revision.slots?.main?.content;

  if (!content) {
    return null;
  }

  return {
    content,
    timestamp: revision.timestamp || new Date().toISOString(),
  };
}

/**
 * Get subpages of a page from MediaWiki.org
 */
async function getSubpages(
  prefix: string,
  namespace: number = 0
): Promise<string[]> {
  const pages: string[] = [];
  let continueToken: string | undefined;

  do {
    const params = new URLSearchParams({
      action: 'query',
      list: 'allpages',
      apprefix: prefix,
      apnamespace: String(namespace),
      aplimit: '500',
      format: 'json',
      formatversion: '2',
    });

    if (continueToken) {
      params.set('apcontinue', continueToken);
    }

    const response = await fetch(`${MEDIAWIKI_API}?${params.toString()}`, {
      headers: { 'User-Agent': USER_AGENT },
    });

    if (!response.ok) {
      break;
    }

    const data = await response.json() as {
      query?: { allpages?: Array<{ title: string }> };
      continue?: { apcontinue?: string };
    };

    if (data.query?.allpages) {
      for (const page of data.query.allpages) {
        pages.push(page.title);
      }
    }

    continueToken = data.continue?.apcontinue;
    await sleep(RATE_LIMIT_MS);
  } while (continueToken);

  return pages;
}

/**
 * Search MediaWiki.org for pages
 */
async function searchMediaWiki(
  query: string,
  namespace: number = 0,
  limit: number = 50
): Promise<Array<{ title: string; snippet: string }>> {
  const params = new URLSearchParams({
    action: 'query',
    list: 'search',
    srsearch: query,
    srnamespace: String(namespace),
    srlimit: String(limit),
    format: 'json',
    formatversion: '2',
  });

  const response = await fetch(`${MEDIAWIKI_API}?${params.toString()}`, {
    headers: { 'User-Agent': USER_AGENT },
  });

  if (!response.ok) {
    return [];
  }

  const data = await response.json() as {
    query?: { search?: Array<{ title: string; snippet: string }> };
  };

  return data.query?.search || [];
}

/**
 * Import extension documentation from mediawiki.org
 *
 * Extensions are documented at Extension:<name> with subpages like:
 * - Extension:CirrusSearch
 * - Extension:CirrusSearch/Setup
 * - Extension:CirrusSearch/Configuration
 */
export async function fetchExtensionDocs(
  extensionName: string,
  options: {
    includeSubpages?: boolean;
    onProgress?: ProgressCallback;
  } = {}
): Promise<{ info: ExtensionDocInfo; pages: ExtensionDocPage[] }> {
  const pages: ExtensionDocPage[] = [];
  const errors: string[] = [];
  const now = new Date();
  const expiresAt = new Date(now.getTime() + DOCS_CACHE_TTL_DAYS * 24 * 60 * 60 * 1000);

  // Normalize extension name
  const normalizedName = extensionName.replace(/^Extension:/, '');
  const mainPageTitle = `Extension:${normalizedName}`;

  // Get list of pages to fetch
  const pagesToFetch: string[] = [mainPageTitle];

  if (options.includeSubpages !== false) {
    options.onProgress?.(0, 1, 'Finding subpages...');
    const subpages = await getSubpages(`Extension:${normalizedName}/`, 0);
    pagesToFetch.push(...subpages);
  }

  options.onProgress?.(0, pagesToFetch.length, `Fetching ${pagesToFetch.length} pages...`);

  // Fetch each page
  for (let i = 0; i < pagesToFetch.length; i++) {
    const title = pagesToFetch[i];
    options.onProgress?.(i + 1, pagesToFetch.length, title);

    try {
      const result = await fetchMediaWikiPage(title);
      if (result) {
        const localPath = `docs/extensions/${normalizedName}/${title.replace(/[/\\:*?"<>|]/g, '_')}.wiki`;
        pages.push({
          pageTitle: title,
          localPath,
          content: result.content,
          contentHash: computeHash(result.content),
          fetchedAt: result.timestamp,
        });
      }
    } catch (error) {
      errors.push(`Failed to fetch ${title}: ${error instanceof Error ? error.message : String(error)}`);
    }

    await sleep(RATE_LIMIT_MS);
  }

  // Try to extract version from main page
  let version: string | null = null;
  const mainPage = pages.find(p => p.pageTitle === mainPageTitle);
  if (mainPage) {
    const versionMatch = mainPage.content.match(/\|\s*version\s*=\s*([^\n|]+)/i);
    if (versionMatch) {
      version = versionMatch[1].trim();
    }
  }

  const info: ExtensionDocInfo = {
    extensionName: normalizedName,
    sourceWiki: 'mediawiki.org',
    version,
    pagesCount: pages.length,
    fetchedAt: now.toISOString(),
    expiresAt: expiresAt.toISOString(),
  };

  return { info, pages };
}

/**
 * Technical doc types and their prefixes on mediawiki.org
 */
export const TECHNICAL_DOC_TYPES = {
  hooks: {
    mainPage: 'Manual:Hooks',
    subpagePrefix: 'Manual:Hooks/',
    searchPrefix: 'Manual:Hooks',
  },
  config: {
    mainPage: 'Manual:Configuration settings',
    subpagePrefix: 'Manual:$wg',
    searchPrefix: '$wg',
  },
  api: {
    mainPage: 'API:Main page',
    subpagePrefix: 'API:',
    searchPrefix: 'API:',
  },
  manual: {
    mainPage: 'Manual:Contents',
    subpagePrefix: 'Manual:',
    searchPrefix: 'Manual:',
  },
} as const;

export type TechnicalDocType = keyof typeof TECHNICAL_DOC_TYPES;

/**
 * Import technical documentation from mediawiki.org
 *
 * Technical docs include:
 * - Manual:Hooks and subpages (hook documentation)
 * - Manual:$wg* configuration variables
 * - API:* pages (API documentation)
 * - Manual:* pages (general manual)
 */
export async function fetchTechnicalDocs(
  docType: TechnicalDocType,
  options: {
    pageTitle?: string;
    includeSubpages?: boolean;
    limit?: number;
    onProgress?: ProgressCallback;
  } = {}
): Promise<TechnicalDoc[]> {
  const docs: TechnicalDoc[] = [];
  const now = new Date();
  const expiresAt = new Date(now.getTime() + DOCS_CACHE_TTL_DAYS * 24 * 60 * 60 * 1000);

  const typeConfig = TECHNICAL_DOC_TYPES[docType];
  const limit = options.limit || 100;

  // Determine pages to fetch
  let pagesToFetch: string[] = [];

  if (options.pageTitle) {
    // Fetch specific page
    pagesToFetch = [options.pageTitle];
    if (options.includeSubpages) {
      const subpages = await getSubpages(options.pageTitle + '/', 0);
      pagesToFetch.push(...subpages.slice(0, limit));
    }
  } else {
    // Fetch main page and subpages for the doc type
    pagesToFetch = [typeConfig.mainPage];
    if (options.includeSubpages !== false) {
      options.onProgress?.(0, 1, `Finding ${docType} documentation pages...`);
      const subpages = await getSubpages(typeConfig.subpagePrefix, 0);
      pagesToFetch.push(...subpages.slice(0, limit));
    }
  }

  options.onProgress?.(0, pagesToFetch.length, `Fetching ${pagesToFetch.length} pages...`);

  // Fetch each page
  for (let i = 0; i < pagesToFetch.length; i++) {
    const title = pagesToFetch[i];
    options.onProgress?.(i + 1, pagesToFetch.length, title);

    try {
      const result = await fetchMediaWikiPage(title);
      if (result) {
        const localPath = `docs/technical/${docType}/${title.replace(/[/\\:*?"<>|]/g, '_')}.wiki`;
        docs.push({
          docType,
          pageTitle: title,
          localPath,
          content: result.content,
          contentHash: computeHash(result.content),
          fetchedAt: result.timestamp,
          expiresAt: expiresAt.toISOString(),
        });
      }
    } catch {
      // Skip failed pages silently
    }

    await sleep(RATE_LIMIT_MS);
  }

  return docs;
}

/**
 * Search for extension documentation
 */
export async function searchExtensions(
  query: string,
  limit: number = 20
): Promise<Array<{ name: string; title: string; snippet: string }>> {
  const results = await searchMediaWiki(`Extension: ${query}`, 0, limit);
  return results
    .filter(r => r.title.startsWith('Extension:'))
    .map(r => ({
      name: r.title.replace('Extension:', '').split('/')[0],
      title: r.title,
      snippet: r.snippet.replace(/<[^>]+>/g, ''), // Strip HTML
    }));
}

/**
 * Get list of installed extensions from LocalSettings.php
 */
export async function getInstalledExtensions(
  localSettingsPath: string
): Promise<string[]> {
  const { readFileSync } = await import('node:fs');
  const { existsSync } = await import('node:fs');

  if (!existsSync(localSettingsPath)) {
    return [];
  }

  const content = readFileSync(localSettingsPath, 'utf-8');
  const extensions: string[] = [];

  // Match wfLoadExtension('ExtensionName')
  const wfLoadPattern = /wfLoadExtension\s*\(\s*['"]([^'"]+)['"]\s*\)/g;
  let match;
  while ((match = wfLoadPattern.exec(content)) !== null) {
    extensions.push(match[1]);
  }

  // Match wfLoadExtensions(['Ext1', 'Ext2'])
  const wfLoadMultiPattern = /wfLoadExtensions\s*\(\s*\[\s*([^\]]+)\s*\]\s*\)/g;
  while ((match = wfLoadMultiPattern.exec(content)) !== null) {
    const exts = match[1].match(/['"]([^'"]+)['"]/g);
    if (exts) {
      extensions.push(...exts.map(e => e.replace(/['"]/g, '')));
    }
  }

  return [...new Set(extensions)].sort();
}

/**
 * Check if documentation is expired
 */
export function isExpired(expiresAt: string): boolean {
  return new Date(expiresAt) < new Date();
}

/**
 * Format expiration time for display
 */
export function formatExpiration(expiresAt: string): string {
  const expires = new Date(expiresAt);
  const now = new Date();
  const diffMs = expires.getTime() - now.getTime();

  if (diffMs < 0) {
    return 'expired';
  }

  const diffDays = Math.floor(diffMs / (24 * 60 * 60 * 1000));
  if (diffDays > 0) {
    return `expires in ${diffDays} day${diffDays !== 1 ? 's' : ''}`;
  }

  const diffHours = Math.floor(diffMs / (60 * 60 * 1000));
  if (diffHours > 0) {
    return `expires in ${diffHours} hour${diffHours !== 1 ? 's' : ''}`;
  }

  return 'expires soon';
}
