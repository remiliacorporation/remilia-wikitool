/**
 * External Wiki Client
 *
 * Consolidates external wiki access into a single module.
 * Uses native fetch() (no node-fetch dependency).
 * Complies with Wikimedia User-Agent policy.
 * Supports Wikipedia, MediaWiki.org, Commons, Wikidata, Miraheze, Fandom, and custom domains.
 */

/** User-Agent per Wikimedia policy */
const USER_AGENT = 'RemiliaWikiBot/1.0 (https://wiki.remilia.org; contact@remilia.org)';

/**
 * Rate Limiter - Enforces minimum delay between requests
 * Wikimedia policy: max 200 requests/second, but we use 100ms (10/sec) to be conservative
 */
class RateLimiter {
  private lastRequest = 0;
  private queue: Array<() => void> = [];
  private processing = false;

  constructor(private minDelayMs: number = 100) {}

  async acquire(): Promise<void> {
    return new Promise(resolve => {
      this.queue.push(resolve);
      this.processQueue();
    });
  }

  private async processQueue(): Promise<void> {
    if (this.processing || this.queue.length === 0) return;
    this.processing = true;

    while (this.queue.length > 0) {
      const now = Date.now();
      const elapsed = now - this.lastRequest;
      if (elapsed < this.minDelayMs) {
        await new Promise(r => setTimeout(r, this.minDelayMs - elapsed));
      }
      this.lastRequest = Date.now();
      const next = this.queue.shift();
      if (next) next();
    }

    this.processing = false;
  }

  /** Adjust rate for different wikis */
  setDelay(ms: number): void {
    this.minDelayMs = ms;
  }
}

// Global rate limiter instance (shared across all requests)
const rateLimiter = new RateLimiter(100);

// Wikimedia domains get stricter rate limiting
const WIKIMEDIA_DOMAINS = new Set([
  'wikipedia.org', 'mediawiki.org', 'wikimedia.org', 'wikidata.org',
  'wiktionary.org', 'wikiquote.org', 'wikibooks.org', 'wikisource.org',
]);

function isWikimediaDomain(domain: string): boolean {
  return [...WIKIMEDIA_DOMAINS].some(wd => domain.endsWith(wd));
}

/** Supported wiki configurations */
export const WIKI_CONFIGS = {
  wikipedia: (lang = 'en') => ({
    name: 'Wikipedia',
    api: `https://${lang}.wikipedia.org/w/api.php`,
    base: `https://${lang}.wikipedia.org/wiki/`,
    lang,
  }),
  mediawiki: {
    name: 'MediaWiki.org',
    api: 'https://www.mediawiki.org/w/api.php',
    base: 'https://www.mediawiki.org/wiki/',
  },
  commons: {
    name: 'Wikimedia Commons',
    api: 'https://commons.wikimedia.org/w/api.php',
    base: 'https://commons.wikimedia.org/wiki/',
  },
  wikidata: {
    name: 'Wikidata',
    api: 'https://www.wikidata.org/w/api.php',
    base: 'https://www.wikidata.org/wiki/',
  },
  // Custom wikis via URL
  custom: (domain: string) => ({
    name: domain,
    api: `https://${domain}/w/api.php`,
    base: `https://${domain}/wiki/`,
  }),
} as const;

export type WikiId = 'wikipedia' | 'mediawiki' | 'commons' | 'wikidata';

/** URL patterns for automatic wiki detection (broad MediaWiki coverage) */
const URL_PATTERNS = [
  // Wikipedia (article + index.php)
  { pattern: /^https?:\/\/(\w+)\.wikipedia\.org\/wiki\/(.+)$/,
    wiki: 'wikipedia' as const,
    extract: (m: RegExpMatchArray) => ({
      lang: m[1],
      domain: `${m[1]}.wikipedia.org`,
      title: m[2],
      api: `https://${m[1]}.wikipedia.org/w/api.php`,
      base: `https://${m[1]}.wikipedia.org/wiki/`,
    }) },
  { pattern: /^https?:\/\/(\w+)\.wikipedia\.org\/w\/index\.php\?title=(.+)/,
    wiki: 'wikipedia' as const,
    extract: (m: RegExpMatchArray) => ({
      lang: m[1],
      domain: `${m[1]}.wikipedia.org`,
      title: m[2],
      api: `https://${m[1]}.wikipedia.org/w/api.php`,
      base: `https://${m[1]}.wikipedia.org/wiki/`,
    }) },

  // MediaWiki.org / Commons / Wikidata
  { pattern: /^https?:\/\/www\.mediawiki\.org\/wiki\/(.+)$/,
    wiki: 'mediawiki' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: 'www.mediawiki.org',
      title: m[1],
      api: 'https://www.mediawiki.org/w/api.php',
      base: 'https://www.mediawiki.org/wiki/',
    }) },
  { pattern: /^https?:\/\/commons\.wikimedia\.org\/wiki\/(.+)$/,
    wiki: 'commons' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: 'commons.wikimedia.org',
      title: m[1],
      api: 'https://commons.wikimedia.org/w/api.php',
      base: 'https://commons.wikimedia.org/wiki/',
    }) },
  { pattern: /^https?:\/\/www\.wikidata\.org\/wiki\/(.+)$/,
    wiki: 'wikidata' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: 'www.wikidata.org',
      title: m[1],
      api: 'https://www.wikidata.org/w/api.php',
      base: 'https://www.wikidata.org/wiki/',
    }) },

  // Miraheze / Fandom / generic MediaWiki
  { pattern: /^https?:\/\/([^/]+\.miraheze\.org)\/wiki\/(.+)$/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/w/api.php`,
      base: `https://${m[1]}/wiki/`,
    }) },
  { pattern: /^https?:\/\/([^/]+\.fandom\.com)\/wiki\/(.+)$/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/api.php`, // Fandom uses /api.php
      base: `https://${m[1]}/wiki/`,
    }) },
  // Generic /wiki/ pattern (custom MediaWiki)
  { pattern: /^https?:\/\/([^/]+)\/wiki\/(.+)$/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/w/api.php`,
      base: `https://${m[1]}/wiki/`,
    }) },
  { pattern: /^https?:\/\/([^/]+)\/w\/index\.php\?title=(.+)/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/w/api.php`,
      base: `https://${m[1]}/wiki/`,
    }) },
  { pattern: /^https?:\/\/([^/]+)\/index\.php\?title=(.+)/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/api.php`,
      base: `https://${m[1]}/wiki/`,
    }) },
  // Fallback for short URLs (e.g., https://wowdev.wiki/M2 - no /wiki/ prefix)
  // Must be last as it's very broad
  { pattern: /^https?:\/\/([^/]+)\/([^/?#]+)$/,
    wiki: 'custom' as const,
    extract: (m: RegExpMatchArray) => ({
      domain: m[1],
      title: m[2],
      api: `https://${m[1]}/api.php`,  // Short URL wikis typically use /api.php
      base: `https://${m[1]}/`,
    }) },
];

export interface ParsedWikiUrl {
  wiki: WikiId | 'custom';
  domain: string;      // e.g. en.wikipedia.org, www.mediawiki.org, foo.miraheze.org
  title: string;
  lang?: string;
  apiUrl: string;
  baseUrl: string;
}

/**
 * Parse a wiki URL into components
 * Returns null if URL is not a MediaWiki-style page.
 */
export function parseWikiUrl(url: string): ParsedWikiUrl | null {
  for (const { pattern, wiki, extract } of URL_PATTERNS) {
    const match = url.match(pattern);
    if (match) {
      const extracted = extract(match);
      const title = decodeURIComponent(extracted.title.split('&')[0].replace(/_/g, ' '));
      return {
        wiki,
        domain: extracted.domain,
        title,
        lang: (extracted as { lang?: string }).lang,
        apiUrl: extracted.api,
        baseUrl: extracted.base,
      };
    }
  }
  return null;
}

/**
 * Make API request with proper User-Agent and rate limiting
 */
async function apiRequest(
  apiUrl: string,
  params: Record<string, string>,
  options: { skipRateLimit?: boolean } = {}
): Promise<unknown> {
  // Enforce rate limiting (unless explicitly skipped for retries)
  if (!options.skipRateLimit) {
    await rateLimiter.acquire();
  }

  const url = `${apiUrl}?${new URLSearchParams({ ...params, format: 'json', formatversion: '2' })}`;

  const response = await fetch(url, {
    headers: {
      'User-Agent': USER_AGENT,
      'Accept': 'application/json',
    },
  });

  if (!response.ok) {
    throw new Error(`API request failed: ${response.status} ${response.statusText}`);
  }

  const data = await response.json() as { error?: { code: string; info: string } };

  if (data.error) {
    throw new Error(`API error: ${data.error.code} - ${data.error.info}`);
  }

  return data;
}

export interface ExternalFetchResult {
  title: string;
  content: string;
  timestamp: string;
  extract?: string;
  url: string;
  sourceWiki?: string;
  sourceDomain?: string;
  contentFormat?: 'wikitext' | 'html' | 'text' | 'markdown';
}

/**
 * Fetch page content from external wiki (known wiki id)
 */
export async function fetchPage(
  title: string,
  wiki: WikiId = 'wikipedia',
  options: { lang?: string; format?: 'wikitext' | 'html' } = {}
): Promise<ExternalFetchResult | null> {
  const config = wiki === 'wikipedia'
    ? WIKI_CONFIGS.wikipedia(options.lang || 'en')
    : WIKI_CONFIGS[wiki];

  return fetchPageFromApi(title, config.api, config.base, {
    format: options.format,
    sourceWiki: wiki,
    sourceDomain: new URL(config.base).host,
  });
}

/**
 * Fetch a MediaWiki page using explicit api/base URLs (custom domains)
 */
async function fetchPageFromApi(
  title: string,
  apiUrl: string,
  baseUrl: string,
  options: { format?: 'wikitext' | 'html'; sourceWiki?: string; sourceDomain?: string } = {}
): Promise<ExternalFetchResult | null> {
  const params: Record<string, string> = {
    action: 'query',
    titles: title,
    prop: 'revisions|extracts',
    rvprop: 'content|timestamp',
    rvslots: 'main',
    exintro: '1',
    explaintext: '1',
  };

  if (options.format === 'html') {
    params.rvparse = '1';
  }

  const data = await apiRequest(apiUrl, params) as {
    query?: {
      pages?: Array<{
        title: string;
        missing?: boolean;
        revisions?: Array<{
          slots?: { main?: { content?: string } };
          timestamp?: string;
        }>;
        extract?: string;
      }>;
    };
  };

  const page = data.query?.pages?.[0];
  if (!page || page.missing) return null;

  const revision = page.revisions?.[0];
  const content = revision?.slots?.main?.content;
  if (!content) return null;

  return {
    title: page.title,
    content,
    timestamp: revision?.timestamp || new Date().toISOString(),
    extract: page.extract,
    url: baseUrl + encodeURIComponent(page.title.replace(/ /g, '_')),
    sourceWiki: options.sourceWiki,
    sourceDomain: options.sourceDomain,
    contentFormat: options.format || 'wikitext',
  };
}

/**
 * Fetch by URL (MediaWiki if recognized, otherwise generic web fetch)
 * For custom/unknown domains, tries multiple API paths before falling back to webfetch
 */
export async function fetchPageByUrl(
  url: string,
  options: { format?: 'wikitext' | 'html'; maxBytes?: number } = {}
): Promise<ExternalFetchResult | null> {
  const parsed = parseWikiUrl(url);

  if (parsed) {
    // Known wiki types use their known API path
    if (parsed.wiki !== 'custom') {
      return fetchPageFromApi(parsed.title, parsed.apiUrl, parsed.baseUrl, {
        format: options.format,
        sourceWiki: parsed.wiki,
        sourceDomain: parsed.domain,
      });
    }

    // Custom domains: try API path detection with retry
    try {
      return await fetchPageWithRetry(parsed.title, parsed.domain, parsed.baseUrl, {
        format: options.format,
        preferredApiUrl: parsed.apiUrl, // Try extracted path first
      });
    } catch (err) {
      // All API paths failed, fall through to webfetch
      console.warn(`MediaWiki API failed for ${parsed.domain}, trying webfetch`);
    }
  }

  // Fallback: generic web fetch (text only)
  return fetchWebUrl(url, { maxBytes: options.maxBytes });
}

/**
 * Fetch page with API path retry for custom domains
 */
async function fetchPageWithRetry(
  title: string,
  domain: string,
  baseUrl: string,
  options: { format?: 'wikitext' | 'html'; preferredApiUrl?: string } = {}
): Promise<ExternalFetchResult | null> {
  const params: Record<string, string> = {
    action: 'query',
    titles: title,
    prop: 'revisions|extracts',
    rvprop: 'content|timestamp',
    rvslots: 'main',
    exintro: '1',
    explaintext: '1',
  };

  if (options.format === 'html') {
    params.rvparse = '1';
  }

  // Build API path list: preferred first, then standard paths
  // Try /api.php before /w/api.php since many custom wikis use the shorter path
  const apiPaths = options.preferredApiUrl
    ? [options.preferredApiUrl, `https://${domain}/api.php`, `https://${domain}/w/api.php`]
    : [`https://${domain}/api.php`, `https://${domain}/w/api.php`];

  // Dedupe paths
  const uniquePaths = [...new Set(apiPaths)];

  let lastError: Error | null = null;

  for (const apiUrl of uniquePaths) {
    try {
      const data = await apiRequest(apiUrl, params) as {
        query?: {
          pages?: Array<{
            title: string;
            missing?: boolean;
            revisions?: Array<{
              slots?: { main?: { content?: string } };
              timestamp?: string;
            }>;
            extract?: string;
          }>;
        };
      };

      const page = data.query?.pages?.[0];
      if (!page || page.missing) return null;

      const revision = page.revisions?.[0];
      const content = revision?.slots?.main?.content;
      if (!content) return null;

      return {
        title: page.title,
        content,
        timestamp: revision?.timestamp || new Date().toISOString(),
        extract: page.extract,
        url: baseUrl + encodeURIComponent(page.title.replace(/ /g, '_')),
        sourceWiki: 'custom',
        sourceDomain: domain,
        contentFormat: options.format || 'wikitext',
      };
    } catch (err) {
      lastError = err instanceof Error ? err : new Error(String(err));
      // Continue to next path on 404/connection errors
      if (!lastError.message.includes('404') &&
          !lastError.message.includes('ENOTFOUND') &&
          !lastError.message.includes('ECONNREFUSED') &&
          !lastError.message.includes('API request failed')) {
        throw lastError;
      }
    }
  }

  throw lastError || new Error(`No working API path found for ${domain}`);
}

/**
 * Generic web fetch for non-MediaWiki URLs (text/HTML only)
 */
export async function fetchWebUrl(
  url: string,
  options: { maxBytes?: number } = {}
): Promise<ExternalFetchResult | null> {
  const response = await fetch(url, {
    headers: { 'User-Agent': USER_AGENT, 'Accept': 'text/html, text/plain;q=0.9,*/*;q=0.1' },
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }

  const contentType = response.headers.get('content-type') || '';
  const isText = contentType.includes('text/html') ||
                 contentType.includes('text/plain') ||
                 contentType.includes('text/markdown');
  if (!isText) {
    throw new Error(`Unsupported content-type: ${contentType}`);
  }

  const maxBytes = options.maxBytes ?? 1_000_000; // 1 MB safety limit
  const text = await response.text();
  const content = text.length > maxBytes ? text.slice(0, maxBytes) : text;

  const parsedUrl = new URL(response.url);
  const contentFormat = contentType.includes('text/html') ? 'html'
    : contentType.includes('text/markdown') ? 'markdown'
    : 'text';

  // Extract a cleaner title from the URL path
  let title = response.url;
  const pathParts = parsedUrl.pathname.split('/').filter(Boolean);
  if (pathParts.length > 0) {
    const filename = pathParts[pathParts.length - 1];
    // Remove extension for cleaner title
    title = decodeURIComponent(filename.replace(/\.[^.]+$/, ''));
  }

  return {
    title,
    content,
    timestamp: new Date().toISOString(),
    url: response.url,
    sourceWiki: 'web',
    sourceDomain: parsedUrl.host,
    contentFormat,
  };
}

export interface ExternalSearchResult {
  title: string;
  snippet: string;
  wordcount?: number;
  url: string;
}

/**
 * Search external wiki
 */
export async function searchWiki(
  query: string,
  wiki: WikiId = 'wikipedia',
  options: { lang?: string; limit?: number } = {}
): Promise<ExternalSearchResult[]> {
  const config = wiki === 'wikipedia'
    ? WIKI_CONFIGS.wikipedia(options.lang || 'en')
    : WIKI_CONFIGS[wiki];

  const data = await apiRequest(config.api, {
    action: 'query',
    list: 'search',
    srsearch: query,
    srlimit: String(options.limit || 10),
    srprop: 'snippet|titlesnippet|wordcount',
  }) as {
    query?: {
      search?: Array<{
        title: string;
        snippet: string;
        wordcount?: number;
      }>;
    };
  };

  const results = data.query?.search || [];

  return results.map(r => ({
    title: r.title,
    snippet: r.snippet.replace(/<[^>]+>/g, ''),
    wordcount: r.wordcount,
    url: config.base + encodeURIComponent(r.title.replace(/ /g, '_')),
  }));
}

/**
 * Search a custom MediaWiki domain
 */
export async function searchWikiByDomain(
  domain: string,
  query: string,
  options: { apiUrl?: string; limit?: number } = {}
): Promise<ExternalSearchResult[]> {
  const apiUrl = options.apiUrl || `https://${domain}/w/api.php`;
  const baseUrl = `https://${domain}/wiki/`;

  const data = await apiRequest(apiUrl, {
    action: 'query',
    list: 'search',
    srsearch: query,
    srlimit: String(options.limit || 10),
    srprop: 'snippet|titlesnippet|wordcount',
  }) as {
    query?: {
      search?: Array<{
        title: string;
        snippet: string;
        wordcount?: number;
      }>;
    };
  };

  const results = data.query?.search || [];

  return results.map(r => ({
    title: r.title,
    snippet: r.snippet.replace(/<[^>]+>/g, ''),
    wordcount: r.wordcount,
    url: baseUrl + encodeURIComponent(r.title.replace(/ /g, '_')),
  }));
}

/**
 * List subpages of a given page title
 * Uses allpages API with prefix filter
 */
export async function listSubpages(
  parentTitle: string,
  domain: string,
  options: { apiUrl?: string; limit?: number } = {}
): Promise<string[]> {
  // Try different API paths
  const apiPaths = options.apiUrl
    ? [options.apiUrl]
    : [`https://${domain}/api.php`, `https://${domain}/w/api.php`];

  const prefix = parentTitle + '/';
  const limit = options.limit || 500;

  for (const apiUrl of apiPaths) {
    try {
      const data = await apiRequest(apiUrl, {
        action: 'query',
        list: 'allpages',
        apprefix: prefix,
        aplimit: String(limit),
      }) as {
        query?: {
          allpages?: Array<{ title: string }>;
        };
      };

      return (data.query?.allpages || []).map(p => p.title);
    } catch (err) {
      // Try next path
      continue;
    }
  }

  return [];
}

/**
 * Fetch multiple pages by title from a domain
 */
export async function fetchPagesByTitles(
  titles: string[],
  domain: string,
  options: { apiUrl?: string; format?: 'wikitext' | 'html' } = {}
): Promise<ExternalFetchResult[]> {
  const results: ExternalFetchResult[] = [];

  // Fetch pages one by one (could batch with titles= but simpler this way)
  for (const title of titles) {
    try {
      const result = await fetchPageWithRetry(
        title,
        domain,
        `https://${domain}/`,
        { format: options.format, preferredApiUrl: options.apiUrl }
      );
      if (result) {
        results.push(result);
      }
    } catch (err) {
      // Skip failed pages
      console.warn(`Failed to fetch ${title}: ${err}`);
    }
  }

  return results;
}

/**
 * Set rate limiter delay (for testing or custom domains)
 */
export function setRateLimitDelay(ms: number): void {
  rateLimiter.setDelay(ms);
}
