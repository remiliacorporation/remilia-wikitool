/**
 * MediaWiki API Client
 *
 * Rate-limited HTTP client with batching support for the MediaWiki API.
 */

import type {
  ApiResponse,
  QueryResponse,
  PageInfo,
  AllPagesItem,
  CategoryMember,
  SearchResult,
  RecentChange,
  LoginResponse,
  EditResponse,
  DeleteResponse,
  BatchOptions,
  PageContent,
  PageTimestamp,
  QueryPageResult,
} from './types.js';

/** Client configuration */
export interface ClientConfig {
  /** Wiki API URL (e.g., https://wiki.remilia.org/api.php) */
  apiUrl: string;
  /** User agent string */
  userAgent?: string;
  /** Rate limit for read operations (ms between requests) */
  rateLimitReadMs?: number;
  /** Rate limit for write operations (ms between requests) */
  rateLimitWriteMs?: number;
  /** Maximum batch size for read operations */
  batchSizeRead?: number;
  /** Maximum batch size for write operations */
  batchSizeWrite?: number;
  /** Request timeout (ms) */
  timeoutMs?: number;
  /** Max retries for read requests */
  maxRetries?: number;
  /** Max retries for write requests */
  maxWriteRetries?: number;
  /** Base retry delay (ms) */
  retryDelayMs?: number;
}

/** Default configuration */
const DEFAULT_CONFIG: Required<Omit<ClientConfig, 'apiUrl'>> = {
  userAgent: 'Wikitool/1.0 (https://wiki.remilia.org)',
  rateLimitReadMs: 300,
  rateLimitWriteMs: 1000,
  batchSizeRead: 500,
  batchSizeWrite: 50,
  timeoutMs: 30000,
  maxRetries: 2,
  maxWriteRetries: 1,
  retryDelayMs: 500,
};

/**
 * Sleep for a given number of milliseconds
 */
function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

/**
 * MediaWiki API Client
 */
export class MediaWikiClient {
  private config: Required<ClientConfig>;
  private lastRequestTime: number = 0;
  private requestCount: number = 0;
  private csrfToken: string | null = null;
  private isLoggedIn: boolean = false;
  private cookies: Map<string, string> = new Map();

  constructor(config: ClientConfig) {
    this.config = {
      ...DEFAULT_CONFIG,
      ...config,
    };
  }

  /**
   * Apply rate limiting before a request
   */
  private async rateLimit(isWrite: boolean = false): Promise<void> {
    const delay = isWrite ? this.config.rateLimitWriteMs : this.config.rateLimitReadMs;
    const elapsed = Date.now() - this.lastRequestTime;

    if (this.requestCount > 0 && elapsed < delay) {
      await sleep(delay - elapsed);
    }

    this.lastRequestTime = Date.now();
    this.requestCount++;
  }

  /**
   * Build cookie header from stored cookies
   */
  private getCookieHeader(): string {
    const parts: string[] = [];
    for (const [key, value] of this.cookies) {
      parts.push(`${key}=${value}`);
    }
    return parts.join('; ');
  }

  /**
   * Parse and store cookies from response
   */
  private storeCookies(response: Response): void {
    const setCookie = response.headers.get('set-cookie');
    if (setCookie) {
      // Parse multiple cookies (they may be combined or in multiple headers)
      const cookieStrings = setCookie.split(/,(?=\s*\w+=)/);
      for (const cookieStr of cookieStrings) {
        const match = cookieStr.match(/^([^=]+)=([^;]*)/);
        if (match) {
          this.cookies.set(match[1].trim(), match[2].trim());
        }
      }
    }
  }

  /**
   * Determine max retries for a request
   */
  private getMaxRetries(isWrite: boolean): number {
    return isWrite ? this.config.maxWriteRetries : this.config.maxRetries;
  }

  /**
   * Determine retry delay with exponential backoff + jitter
   */
  private getRetryDelayMs(attempt: number): number {
    const base = this.config.retryDelayMs * Math.pow(2, attempt);
    const jitter = Math.floor(Math.random() * 100);
    return base + jitter;
  }

  /**
   * Determine if a status code is retryable
   */
  private isRetryableStatus(status: number): boolean {
    return status === 408 || status === 429 || status === 502 || status === 503 || status === 504;
  }

  /**
   * Determine if a thrown error is retryable
   */
  private isRetryableError(error: unknown, isWrite: boolean): boolean {
    if (!(error instanceof Error)) return !isWrite;
    if (error.name === 'AbortError') return true;
    const message = error.message || '';
    return /ECONNRESET|ETIMEDOUT|EAI_AGAIN|ENOTFOUND/i.test(message) && !isWrite ? true : !isWrite;
  }

  /**
   * Make a request with retry/backoff and timeout
   */
  private async request(
    method: 'GET' | 'POST',
    params: Record<string, string | number | undefined>,
    isWrite: boolean
  ): Promise<unknown> {
    const maxRetries = this.getMaxRetries(isWrite);

    for (let attempt = 0; attempt <= maxRetries; attempt++) {
      await this.rateLimit(isWrite);

      const headers: Record<string, string> = {
        'User-Agent': this.config.userAgent,
      };

      if (this.cookies.size > 0) {
        headers['Cookie'] = this.getCookieHeader();
      }

      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), this.config.timeoutMs);

      try {
        let response: Response;

        if (method === 'GET') {
          const url = new URL(this.config.apiUrl);
          url.searchParams.set('format', 'json');
          url.searchParams.set('formatversion', '2');

          for (const [key, value] of Object.entries(params)) {
            if (value !== undefined) {
              url.searchParams.set(key, String(value));
            }
          }

          response = await fetch(url.toString(), { headers, signal: controller.signal });
        } else {
          const body = new URLSearchParams();
          body.set('format', 'json');
          body.set('formatversion', '2');

          for (const [key, value] of Object.entries(params)) {
            if (value !== undefined) {
              body.set(key, String(value));
            }
          }

          headers['Content-Type'] = 'application/x-www-form-urlencoded';

          response = await fetch(this.config.apiUrl, {
            method: 'POST',
            headers,
            body: body.toString(),
            signal: controller.signal,
          });
        }

        clearTimeout(timeout);
        this.storeCookies(response);

        if (!response.ok) {
          if (attempt < maxRetries && this.isRetryableStatus(response.status)) {
            await sleep(this.getRetryDelayMs(attempt));
            continue;
          }
          throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }

        return await response.json();
      } catch (error) {
        clearTimeout(timeout);
        if (attempt < maxRetries && this.isRetryableError(error, isWrite)) {
          await sleep(this.getRetryDelayMs(attempt));
          continue;
        }
        throw error;
      }
    }

    throw new Error('Request failed after retries');
  }

  /**
   * Make a GET request to the API
   */
  async get(params: Record<string, string | number | undefined>): Promise<unknown> {
    return this.request('GET', params, false);
  }

  /**
   * Make a POST request to the API
   */
  async post(params: Record<string, string | number | undefined>): Promise<unknown> {
    return this.request('POST', params, true);
  }

  // =========================================================================
  // Authentication
  // =========================================================================

  /**
   * Login with bot credentials
   */
  async login(username: string, password: string): Promise<void> {
    // Get login token
    const tokenResult = await this.get({
      action: 'query',
      meta: 'tokens',
      type: 'login',
    }) as QueryResponse;

    const loginToken = tokenResult.query?.tokens?.logintoken;
    if (!loginToken) {
      throw new Error('Failed to get login token');
    }

    // Perform login
    const loginResult = await this.post({
      action: 'login',
      lgname: username,
      lgpassword: password,
      lgtoken: loginToken,
    }) as LoginResponse;

    if (loginResult.login?.result !== 'Success') {
      throw new Error(`Login failed: ${loginResult.login?.reason || 'Unknown error'}`);
    }

    this.isLoggedIn = true;
    this.csrfToken = null; // Clear cached token, will be fetched on demand
  }

  /**
   * Get CSRF token (required for edit/delete operations)
   */
  async getCsrfToken(): Promise<string> {
    if (this.csrfToken) {
      return this.csrfToken;
    }

    const result = await this.get({
      action: 'query',
      meta: 'tokens',
    });

    const token = (result as QueryResponse).query?.tokens?.csrftoken;
    if (!token) {
      throw new Error('Failed to get CSRF token');
    }

    this.csrfToken = token;
    return token;
  }

  /**
   * Check if logged in
   */
  get loggedIn(): boolean {
    return this.isLoggedIn;
  }

  // =========================================================================
  // Page queries
  // =========================================================================

  /**
   * Get all pages in a namespace
   */
  async getAllPages(namespace: number, options: BatchOptions = {}): Promise<string[]> {
    const pages: string[] = [];
    let continueToken: string | undefined;

    const batchSize = options.batchSize || this.config.batchSizeRead;

    do {
      const result = await this.get({
        action: 'query',
        list: 'allpages',
        apnamespace: namespace,
        aplimit: batchSize,
        apcontinue: continueToken,
      });

      const query = (result as QueryResponse).query;
      const cont = (result as QueryResponse).continue;

      if (query?.allpages) {
        for (const page of query.allpages) {
          pages.push(page.title);
        }
      }

      continueToken = cont?.apcontinue;
      options.onProgress?.(pages.length, -1);
    } while (continueToken);

    return pages;
  }

  /**
   * Get pages in a category
   */
  async getCategoryMembers(category: string, options: BatchOptions = {}): Promise<string[]> {
    const pages: string[] = [];
    let continueToken: string | undefined;

    const categoryTitle = category.startsWith('Category:') ? category : `Category:${category}`;
    const batchSize = options.batchSize || this.config.batchSizeRead;

    do {
      const result = await this.get({
        action: 'query',
        list: 'categorymembers',
        cmtitle: categoryTitle,
        cmlimit: batchSize,
        cmtype: 'page',
        cmcontinue: continueToken,
      });

      const query = (result as QueryResponse).query;
      const cont = (result as QueryResponse).continue;

      if (query?.categorymembers) {
        for (const page of query.categorymembers) {
          pages.push(page.title);
        }
      }

      continueToken = cont?.cmcontinue;
      options.onProgress?.(pages.length, -1);
    } while (continueToken);

    return pages;
  }

  /**
   * Get recent changes since a timestamp
   */
  async getRecentChanges(since: string, namespaces: number[] = [0]): Promise<string[]> {
    const pages = new Set<string>();
    let continueToken: string | undefined;

    const nsString = namespaces.join('|');

    do {
      const result = await this.get({
        action: 'query',
        list: 'recentchanges',
        rcstart: since,
        rcdir: 'newer',
        rcnamespace: nsString,
        rcprop: 'title',
        rclimit: this.config.batchSizeRead,
        rctype: 'edit|new',
        rccontinue: continueToken,
      });

      const query = (result as QueryResponse).query;
      const cont = (result as QueryResponse).continue;

      if (query?.recentchanges) {
        for (const change of query.recentchanges) {
          pages.add(change.title);
        }
      }

      continueToken = cont?.rccontinue;
    } while (continueToken);

    return Array.from(pages);
  }

  /**
   * Search pages by content
   */
  async search(query: string, namespaces: number[] = [0], limit: number = 50): Promise<SearchResult[]> {
    const result = await this.get({
      action: 'query',
      list: 'search',
      srsearch: query,
      srnamespace: namespaces.join('|'),
      srlimit: limit,
    });

    return (result as QueryResponse).query?.search || [];
  }

  /**
   * Get querypage results (Special: pages backed by QueryPage)
   */
  async getQueryPageItems(
    page: string,
    options: { limit?: number } = {}
  ): Promise<{ items: QueryPageResult[]; truncated: boolean }> {
    const results: QueryPageResult[] = [];
    const limit = options.limit ?? 200;
    let offset: string | undefined;
    let truncated = false;

    while (true) {
      const remaining = limit === 0 ? 0 : limit - results.length;
      if (limit !== 0 && remaining <= 0) {
        break;
      }

      const qplimit = limit === 0
        ? 'max'
        : Math.min(remaining, 500);

      const result = await this.get({
        action: 'query',
        list: 'querypage',
        qppage: page,
        qplimit,
        qpoffset: offset,
      });

      if ((result as ApiResponse).error) {
        throw new Error(`QueryPage failed: ${(result as ApiResponse).error?.info || 'Unknown error'}`);
      }

      const query = (result as QueryResponse).query;
      const items = query?.querypage?.results ?? [];
      for (const item of items) {
        if (item && typeof item.title === 'string') {
          results.push(item);
        }
      }

      const cont = (result as QueryResponse).continue?.qpoffset;
      if (limit !== 0 && results.length >= limit) {
        if (cont !== undefined) {
          truncated = true;
        }
        break;
      }

      if (cont === undefined) {
        break;
      }

      offset = cont;
    }

    return { items: results, truncated };
  }

  /**
   * Get page content for a single page
   */
  async getPageContent(title: string): Promise<PageContent | null> {
    const result = await this.get({
      action: 'query',
      titles: title,
      prop: 'revisions',
      rvprop: 'content|timestamp|ids',
      rvslots: 'main',
    });

    const pages = (result as QueryResponse).query?.pages;
    if (!pages) return null;

    const pageInfo = Object.values(pages)[0];
    if (!pageInfo || pageInfo.missing || !pageInfo.revisions?.[0]) {
      return null;
    }

    const revision = pageInfo.revisions[0];
    const content = revision.slots?.main?.content;

    if (content === undefined) return null;

    return {
      title: pageInfo.title,
      content,
      timestamp: revision.timestamp,
      revisionId: revision.revid,
      pageId: pageInfo.pageid!,
      contentModel: revision.slots?.main?.contentmodel || 'wikitext',
      namespace: pageInfo.ns,
    };
  }

  /**
   * Get parsed HTML for a page (supports Special: pages)
   */
  async getParsedHtml(title: string): Promise<string | null> {
    const result = await this.get({
      action: 'parse',
      page: title,
      prop: 'text',
    });

    const parse = (result as { parse?: { text?: unknown } }).parse;
    if (!parse || parse.text == null) return null;

    if (typeof parse.text === 'string') {
      return parse.text;
    }

    if (typeof parse.text === 'object' && parse.text) {
      const legacy = parse.text as { '*': unknown };
      if (typeof legacy['*'] === 'string') {
        return legacy['*'];
      }
    }

    return null;
  }

  /**
   * Get page content for multiple pages (batched)
   */
  async getPageContents(titles: string[], options: BatchOptions = {}): Promise<Map<string, PageContent>> {
    const results = new Map<string, PageContent>();
    const batchSize = options.batchSize || 50; // API limit for titles parameter

    for (let i = 0; i < titles.length; i += batchSize) {
      const batch = titles.slice(i, i + batchSize);

      const result = await this.get({
        action: 'query',
        titles: batch.join('|'),
        prop: 'revisions',
        rvprop: 'content|timestamp|ids',
        rvslots: 'main',
      });

      const pages = (result as QueryResponse).query?.pages;
      if (pages) {
        for (const pageInfo of Object.values(pages)) {
          if (pageInfo.missing || !pageInfo.revisions?.[0]) continue;

          const revision = pageInfo.revisions[0];
          const content = revision.slots?.main?.content;

          if (content === undefined) continue;

          results.set(pageInfo.title, {
            title: pageInfo.title,
            content,
            timestamp: revision.timestamp,
            revisionId: revision.revid,
            pageId: pageInfo.pageid!,
            contentModel: revision.slots?.main?.contentmodel || 'wikitext',
            namespace: pageInfo.ns,
          });
        }
      }

      options.onProgress?.(Math.min(i + batchSize, titles.length), titles.length);
    }

    return results;
  }

  /**
   * Get page timestamps (for conflict detection)
   */
  async getPageTimestamps(titles: string[]): Promise<Map<string, PageTimestamp>> {
    const results = new Map<string, PageTimestamp>();
    const batchSize = 50;

    for (let i = 0; i < titles.length; i += batchSize) {
      const batch = titles.slice(i, i + batchSize);

      const result = await this.get({
        action: 'query',
        titles: batch.join('|'),
        prop: 'revisions',
        rvprop: 'timestamp|ids', // Only timestamp, not content
      });

      const pages = (result as QueryResponse).query?.pages;
      if (pages) {
        for (const pageInfo of Object.values(pages)) {
          if (pageInfo.missing || !pageInfo.revisions?.[0]) continue;

          const revision = pageInfo.revisions[0];
          results.set(pageInfo.title, {
            title: pageInfo.title,
            timestamp: revision.timestamp,
            revisionId: revision.revid,
          });
        }
      }
    }

    return results;
  }

  // =========================================================================
  // Write operations
  // =========================================================================

  /**
   * Edit a page
   */
  async editPage(
    title: string,
    content: string,
    summary: string,
    options: { contentModel?: string; minor?: boolean; bot?: boolean } = {}
  ): Promise<EditResponse['edit']> {
    const token = await this.getCsrfToken();

    const params: Record<string, string | number | undefined> = {
      action: 'edit',
      title,
      text: content,
      summary,
      token,
    };

    if (options.contentModel) {
      params.contentmodel = options.contentModel;
    }
    if (options.minor) {
      params.minor = 1;
    }
    if (options.bot) {
      params.bot = 1;
    }

    const result = await this.post(params);

    if ((result as ApiResponse).error) {
      throw new Error(`Edit failed: ${(result as ApiResponse).error?.info}`);
    }

    return (result as EditResponse).edit;
  }

  /**
   * Delete a page
   */
  async deletePage(title: string, reason: string): Promise<DeleteResponse['delete']> {
    const token = await this.getCsrfToken();

    const result = await this.post({
      action: 'delete',
      title,
      reason,
      token,
    });

    if ((result as ApiResponse).error) {
      const error = (result as ApiResponse).error!;
      // Handle "already deleted" gracefully
      if (error.code === 'missingtitle') {
        return { title, reason, logid: 0 };
      }
      throw new Error(`Delete failed: ${error.info}`);
    }

    return (result as DeleteResponse).delete;
  }

  // =========================================================================
  // Utility methods
  // =========================================================================

  /**
   * Get the current request count (useful for stats)
   */
  get totalRequests(): number {
    return this.requestCount;
  }

  /**
   * Reset request counter
   */
  resetRequestCount(): void {
    this.requestCount = 0;
  }
}

/**
 * Create a client from environment variables
 */
export function createClientFromEnv(): MediaWikiClient {
  const apiUrl = process.env.WIKI_API_URL || 'https://wiki.remilia.org/api.php';

  return new MediaWikiClient({
    apiUrl,
    rateLimitReadMs: parseInt(process.env.WIKI_RATE_LIMIT_READ || '300'),
    rateLimitWriteMs: parseInt(process.env.WIKI_RATE_LIMIT_WRITE || '1000'),
    timeoutMs: parseInt(process.env.WIKI_HTTP_TIMEOUT_MS || '30000'),
    maxRetries: parseInt(process.env.WIKI_HTTP_RETRIES || '2'),
    maxWriteRetries: parseInt(process.env.WIKI_HTTP_WRITE_RETRIES || '1'),
    retryDelayMs: parseInt(process.env.WIKI_HTTP_RETRY_DELAY_MS || '500'),
  });
}

/**
 * Create a client and log in
 */
export async function createAuthenticatedClient(): Promise<MediaWikiClient> {
  const client = createClientFromEnv();

  const username = process.env.WIKI_BOT_USER;
  const password = process.env.WIKI_BOT_PASS;

  if (!username || !password) {
    throw new Error('WIKI_BOT_USER and WIKI_BOT_PASS environment variables required');
  }

  await client.login(username, password);
  return client;
}
