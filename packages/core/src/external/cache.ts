/**
 * External Cache Layer
 *
 * Caching layer for external wiki content using the fetch_cache table.
 * Supports TTL-based expiration and category tagging.
 */

import type { Database } from '../storage/sqlite.js';
import {
  fetchPage,
  fetchPageByUrl,
  WIKI_CONFIGS,
  type WikiId,
  type ExternalFetchResult,
} from './client.js';

export interface CachedPage extends ExternalFetchResult {
  id: number;
  wiki: string;
  fetched_at: string;
  expires_at: string | null;
  category: string | null;
  tags: string | null;
}

// Type for accessing the internal db handle
interface InternalDb {
  prepare(sql: string): {
    run(...params: unknown[]): { changes: number; lastInsertRowid: number | bigint };
    get(...params: unknown[]): unknown;
    all(...params: unknown[]): unknown[];
  };
}

/**
 * Cache layer for external wiki content
 * Uses existing fetch_cache table with extended columns
 */
export class ExternalCache {
  private internalDb: InternalDb;

  constructor(private db: Database) {
    // Access the internal bun:sqlite database
    this.internalDb = (db as unknown as { db: InternalDb }).db;
  }

  /**
   * Get or fetch a page with caching (known wiki id)
   */
  async getPage(
    title: string,
    wiki: WikiId,
    options: {
      category?: string;
      ttlHours?: number;
      lang?: string;
      format?: 'wikitext' | 'html';
      forceRefresh?: boolean;
    } = {}
  ): Promise<CachedPage | null> {
    const domain = wiki === 'wikipedia'
      ? `${options.lang || 'en'}.wikipedia.org`
      : new URL(WIKI_CONFIGS[wiki].base).host;
    const format = options.format || 'wikitext';

    // Check cache first (unless force refresh)
    if (!options.forceRefresh) {
      const cached = this.getCached(title, wiki, domain, format);
      if (cached && !this.isExpired(cached)) {
        return cached;
      }
    }

    // Fetch and cache
    const result = await fetchPage(title, wiki, { lang: options.lang, format: options.format });
    if (!result) return null;

    return this.cacheResult(result, wiki, domain, options);
  }

  /**
   * Get or fetch by URL (MediaWiki or generic web)
   */
  async getPageByUrl(
    url: string,
    options: {
      category?: string;
      ttlHours?: number;
      format?: 'wikitext' | 'html';
      maxBytes?: number;
      forceRefresh?: boolean;
    } = {}
  ): Promise<CachedPage | null> {
    const result = await fetchPageByUrl(url, { format: options.format, maxBytes: options.maxBytes });
    if (!result) return null;

    const wiki = (result.sourceWiki || 'web') as WikiId | 'custom' | 'web';
    const domain = result.sourceDomain || new URL(result.url).host;
    const category = options.category || (wiki === 'web' ? 'web' : 'external');

    return this.cacheResult(result, wiki, domain, { ...options, category });
  }

  /**
   * Get cached page without fetching
   */
  getCached(title: string, wiki: string, domain: string, format: string): CachedPage | null {
    const stmt = this.internalDb.prepare(`
      SELECT id, source_wiki as wiki, source_domain as sourceDomain, page_title as title,
             content, content_format as contentFormat, fetched_at, expires_at, category, tags
      FROM fetch_cache
      WHERE page_title = ? AND source_wiki = ? AND source_domain = ? AND content_format = ?
    `);
    const row = stmt.get(title, wiki, domain, format) as {
      id: number;
      wiki: string;
      sourceDomain: string;
      title: string;
      content: string;
      contentFormat: string;
      fetched_at: string;
      expires_at: string | null;
      category: string | null;
      tags: string | null;
    } | undefined;

    if (!row) return null;

    return {
      id: row.id,
      wiki: row.wiki,
      title: row.title,
      content: row.content,
      timestamp: row.fetched_at,
      url: '', // URL not stored, would need to reconstruct
      sourceWiki: row.wiki,
      sourceDomain: row.sourceDomain,
      contentFormat: row.contentFormat as 'wikitext' | 'html' | 'text',
      fetched_at: row.fetched_at,
      expires_at: row.expires_at,
      category: row.category,
      tags: row.tags,
    };
  }

  /**
   * Check if cached entry is expired
   */
  isExpired(cached: CachedPage): boolean {
    if (!cached.expires_at) return false;
    return new Date(cached.expires_at) < new Date();
  }

  /**
   * Cache a fetch result
   */
  private cacheResult(
    result: ExternalFetchResult,
    wiki: WikiId | 'custom' | 'web',
    domain: string,
    options: { category?: string; ttlHours?: number }
  ): CachedPage {
    const ttlHours = options.ttlHours ?? 24;
    const expiresAt = new Date(Date.now() + ttlHours * 60 * 60 * 1000).toISOString();

    const stmt = this.internalDb.prepare(`
      INSERT OR REPLACE INTO fetch_cache
      (source_wiki, source_domain, page_title, content, content_format, fetched_at, expires_at, category)
      VALUES (?, ?, ?, ?, ?, datetime('now'), ?, ?)
    `);

    const contentFormat = result.contentFormat || 'wikitext';
    const info = stmt.run(
      wiki,
      domain,
      result.title,
      result.content,
      contentFormat,
      expiresAt,
      options.category || null
    );

    return {
      id: Number(info.lastInsertRowid),
      wiki: String(wiki),
      ...result,
      fetched_at: new Date().toISOString(),
      expires_at: expiresAt,
      category: options.category || null,
      tags: null,
    };
  }

  /**
   * Search cached content
   */
  searchCached(
    query: string,
    options: { wiki?: string; domain?: string; category?: string; limit?: number } = {}
  ): CachedPage[] {
    let sql = `
      SELECT id, source_wiki as wiki, source_domain as sourceDomain, page_title as title,
             content, content_format as contentFormat, fetched_at, expires_at, category, tags
      FROM fetch_cache
      WHERE content LIKE ?
    `;
    const params: unknown[] = [`%${query}%`];

    if (options.wiki) {
      sql += ' AND source_wiki = ?';
      params.push(options.wiki);
    }

    if (options.domain) {
      sql += ' AND source_domain = ?';
      params.push(options.domain);
    }

    if (options.category) {
      sql += ' AND category = ?';
      params.push(options.category);
    }

    sql += ' ORDER BY fetched_at DESC LIMIT ?';
    params.push(options.limit || 20);

    const stmt = this.internalDb.prepare(sql);
    const rows = stmt.all(...params) as Array<{
      id: number;
      wiki: string;
      sourceDomain: string;
      title: string;
      content: string;
      contentFormat: string;
      fetched_at: string;
      expires_at: string | null;
      category: string | null;
      tags: string | null;
    }>;

    return rows.map(row => ({
      id: row.id,
      wiki: row.wiki,
      title: row.title,
      content: row.content,
      timestamp: row.fetched_at,
      url: '',
      sourceWiki: row.wiki,
      sourceDomain: row.sourceDomain,
      contentFormat: row.contentFormat as 'wikitext' | 'html' | 'text',
      fetched_at: row.fetched_at,
      expires_at: row.expires_at,
      category: row.category,
      tags: row.tags,
    }));
  }

  /**
   * Clear expired cache entries
   */
  clearExpired(): number {
    const stmt = this.internalDb.prepare(`
      DELETE FROM fetch_cache WHERE expires_at < datetime('now')
    `);
    return stmt.run().changes;
  }

  /**
   * Clear all cache entries
   */
  clearAll(): number {
    const stmt = this.internalDb.prepare('DELETE FROM fetch_cache');
    return stmt.run().changes;
  }

  /**
   * Get cache statistics
   */
  getStats(): {
    totalEntries: number;
    expiredEntries: number;
    byWiki: Record<string, number>;
    byCategory: Record<string, number>;
  } {
    const total = (this.internalDb.prepare(
      'SELECT COUNT(*) as count FROM fetch_cache'
    ).get() as { count: number }).count;

    const expired = (this.internalDb.prepare(
      "SELECT COUNT(*) as count FROM fetch_cache WHERE expires_at < datetime('now')"
    ).get() as { count: number }).count;

    const byWiki: Record<string, number> = {};
    const wikiRows = this.internalDb.prepare(
      'SELECT source_wiki, COUNT(*) as count FROM fetch_cache GROUP BY source_wiki'
    ).all() as { source_wiki: string; count: number }[];
    for (const row of wikiRows) {
      byWiki[row.source_wiki] = row.count;
    }

    const byCategory: Record<string, number> = {};
    const catRows = this.internalDb.prepare(
      'SELECT category, COUNT(*) as count FROM fetch_cache WHERE category IS NOT NULL GROUP BY category'
    ).all() as { category: string; count: number }[];
    for (const row of catRows) {
      byCategory[row.category] = row.count;
    }

    return {
      totalEntries: total,
      expiredEntries: expired,
      byWiki,
      byCategory,
    };
  }
}
