/**
 * SQLite database wrapper for wikitool
 *
 * Uses Bun's built-in bun:sqlite for fast, synchronous operations.
 */

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { Database as BunDatabase, type SQLQueryBindings } from 'bun:sqlite';
import {
  runMigrations as runMigrationsImpl,
  getSchemaVersion,
  getExpectedVersion,
  validateSchema,
  hasPendingMigrations,
  getPendingMigrations,
  getMigrationHistory,
  type MigrationResult,
  type SchemaValidation,
  type MigrationRecord,
} from './migrations.js';
import { execSql } from './utils.js';

type SqlBinding = SQLQueryBindings;

function toBinding(value: SqlBinding | null | undefined): SqlBinding {
  return value ?? null;
}

/** Sync status for pages */
export type SyncStatus = 'synced' | 'local_modified' | 'wiki_modified' | 'conflict' | 'staged' | 'new';

/** Page record from database */
export interface PageRecord {
  id: number;
  title: string;
  namespace: number;
  page_type: string;
  filename: string;
  filepath: string;
  template_category: string | null;
  content: string | null;
  content_hash: string | null;
  file_mtime: number | null;
  wiki_modified_at: string | null;
  last_synced_at: string | null;
  sync_status: SyncStatus;
  is_redirect: number;
  redirect_target: string | null;
  content_model: string | null;
  page_id: number | null;
  revision_id: number | null;
  created_at: string;
  updated_at: string;
}

/** Config record from database */
export interface ConfigRecord {
  key: string;
  value: string | null;
  updated_at: string;
}

/** Sync log record */
export interface SyncLogRecord {
  id: number;
  operation: 'pull' | 'push' | 'delete' | 'resolve' | 'init';
  page_title: string | null;
  status: 'success' | 'failed' | 'conflict' | 'skipped';
  revision_id: number | null;
  error_message: string | null;
  details: string | null;
  timestamp: string;
}

/**
 * Compute SHA-256 hash of content (first 16 chars)
 */
export function computeHash(content: string): string {
  return createHash('sha256').update(content).digest('hex').slice(0, 16);
}

/**
 * Database wrapper class
 */
export class Database {
  private db: BunDatabase;

  private constructor(db: BunDatabase) {
    this.db = db;
  }

  /**
   * Create a new database instance
   */
  static async create(dbPath: string): Promise<Database> {
    const isMemoryDb = dbPath === ':memory:' || dbPath.startsWith('file::memory');
    if (!isMemoryDb) {
      // Ensure directory exists
      const dir = dirname(dbPath);
      if (!existsSync(dir)) {
        mkdirSync(dir, { recursive: true });
      }
    }

    const db = new BunDatabase(dbPath);
    // Enable WAL mode for better concurrency
    db.run('PRAGMA journal_mode = WAL;');

    const instance = new Database(db);
    await instance.runMigrations();
    return instance;
  }

  /**
   * Get the underlying BunDatabase instance.
   * Use sparingly - prefer using Database methods when possible.
   */
  getRawDb(): BunDatabase {
    return this.db;
  }

  /**
   * Run pending migrations using the enhanced migration runner
   */
  private async runMigrations(): Promise<MigrationResult> {
    return runMigrationsImpl(this.db);
  }

  /**
   * Get current schema version
   */
  getSchemaVersion(): string {
    return getSchemaVersion(this.db);
  }

  /**
   * Get expected (latest) schema version
   */
  getExpectedVersion(): string {
    return getExpectedVersion();
  }

  /**
   * Validate database schema
   */
  validateSchema(): SchemaValidation {
    return validateSchema(this.db);
  }

  /**
   * Check if there are pending migrations
   */
  hasPendingMigrations(): boolean {
    return hasPendingMigrations(this.db);
  }

  /**
   * Get list of pending migration versions
   */
  getPendingMigrations(): string[] {
    return getPendingMigrations(this.db);
  }

  /**
   * Get migration history
   */
  getMigrationHistory(): MigrationRecord[] {
    return getMigrationHistory(this.db);
  }

  /**
   * Run migrations manually (useful for CLI commands)
   */
  runMigrationsManual(): MigrationResult {
    return runMigrationsImpl(this.db);
  }

  /**
   * Execute raw SQL
   */
  exec(sql: string): void {
    execSql(this.db, sql);
  }

  /**
   * Close database connection
   */
  close(): void {
    this.db.close();
  }

  // =========================================================================
  // Configuration operations
  // =========================================================================

  getConfig(key: string): string | null {
    const stmt = this.db.prepare('SELECT value FROM config WHERE key = ?');
    const row = stmt.get(key) as { value: string | null } | undefined;
    return row?.value ?? null;
  }

  setConfig(key: string, value: string | null): void {
    const stmt = this.db.prepare(
      `INSERT INTO config (key, value, updated_at)
       VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at`
    );
    stmt.run(key, value);
  }

  // =========================================================================
  // Page operations
  // =========================================================================

  /**
   * Get a page by title
   */
  getPage(title: string): PageRecord | null {
    const stmt = this.db.prepare('SELECT * FROM pages WHERE title = ?');
    return (stmt.get(title) as PageRecord) ?? null;
  }

  /**
   * Get a page by filepath
   */
  getPageByPath(filepath: string): PageRecord | null {
    const stmt = this.db.prepare('SELECT * FROM pages WHERE filepath = ?');
    return (stmt.get(filepath) as PageRecord) ?? null;
  }

  /**
   * Get a page by filepath (case-insensitive)
   * Used for detecting case collisions on Windows filesystems
   */
  getPageByFilepath(filepath: string): PageRecord | null {
    // Normalize path separators and use LOWER() for case-insensitive match
    const normalizedPath = filepath.replace(/\\/g, '/');
    const stmt = this.db.prepare(
      "SELECT * FROM pages WHERE LOWER(REPLACE(filepath, '\\', '/')) = LOWER(?)"
    );
    return (stmt.get(normalizedPath) as PageRecord) ?? null;
  }

  /**
   * Get all pages with optional filters
   */
  getPages(options: {
    namespace?: number;
    syncStatus?: SyncStatus;
    pageType?: string;
    limit?: number;
    offset?: number;
  } = {}): PageRecord[] {
    let sql = 'SELECT * FROM pages WHERE 1=1';
    const params: SqlBinding[] = [];

    if (options.namespace !== undefined) {
      sql += ' AND namespace = ?';
      params.push(options.namespace);
    }
    if (options.syncStatus) {
      sql += ' AND sync_status = ?';
      params.push(options.syncStatus);
    }
    if (options.pageType) {
      sql += ' AND page_type = ?';
      params.push(options.pageType);
    }

    sql += ' ORDER BY title';

    if (options.limit) {
      sql += ' LIMIT ?';
      params.push(options.limit);
      if (options.offset) {
        sql += ' OFFSET ?';
        params.push(options.offset);
      }
    }

    const stmt = this.db.prepare(sql);
    return stmt.all(...params) as PageRecord[];
  }

  /**
   * Get pages with modified status (local_modified, wiki_modified, conflict)
   */
  getModifiedPages(): PageRecord[] {
    const stmt = this.db.prepare(
      `SELECT * FROM pages WHERE sync_status IN ('local_modified', 'wiki_modified', 'conflict', 'new')
       ORDER BY title`
    );
    return stmt.all() as PageRecord[];
  }

  /**
   * Get all page titles
   */
  getAllTitles(): string[] {
    const stmt = this.db.prepare('SELECT title FROM pages ORDER BY title');
    return (stmt.all() as { title: string }[]).map(r => r.title);
  }

  /**
   * Insert or update a page
   */
  upsertPage(page: Partial<PageRecord> & { title: string }): number {
    const existing = this.getPage(page.title);

    if (existing) {
      // Update existing page
      const stmt = this.db.prepare(`
        UPDATE pages SET
          namespace = COALESCE(?, namespace),
          page_type = COALESCE(?, page_type),
          filename = COALESCE(?, filename),
          filepath = COALESCE(?, filepath),
          template_category = COALESCE(?, template_category),
          content = COALESCE(?, content),
          content_hash = COALESCE(?, content_hash),
          file_mtime = COALESCE(?, file_mtime),
          wiki_modified_at = COALESCE(?, wiki_modified_at),
          last_synced_at = COALESCE(?, last_synced_at),
          sync_status = COALESCE(?, sync_status),
          is_redirect = COALESCE(?, is_redirect),
          redirect_target = ?,
          content_model = COALESCE(?, content_model),
          page_id = COALESCE(?, page_id),
          revision_id = COALESCE(?, revision_id),
          updated_at = datetime('now')
        WHERE title = ?
      `);
      stmt.run(
        toBinding(page.namespace),
        toBinding(page.page_type),
        toBinding(page.filename),
        toBinding(page.filepath),
        toBinding(page.template_category),
        toBinding(page.content),
        toBinding(page.content_hash),
        toBinding(page.file_mtime),
        toBinding(page.wiki_modified_at),
        toBinding(page.last_synced_at),
        toBinding(page.sync_status),
        toBinding(page.is_redirect),
        toBinding(page.redirect_target),
        toBinding(page.content_model),
        toBinding(page.page_id),
        toBinding(page.revision_id),
        page.title
      );
      return existing.id;
    } else {
      // Insert new page
      const stmt = this.db.prepare(`
        INSERT INTO pages (
          title, namespace, page_type, filename, filepath, template_category,
          content, content_hash, file_mtime, wiki_modified_at, last_synced_at,
          sync_status, is_redirect, redirect_target, content_model, page_id, revision_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
      const result = stmt.run(
        page.title,
        page.namespace ?? 0,
        page.page_type ?? 'article',
        page.filename ?? '',
        page.filepath ?? '',
        toBinding(page.template_category),
        toBinding(page.content),
        toBinding(page.content_hash),
        toBinding(page.file_mtime),
        toBinding(page.wiki_modified_at),
        toBinding(page.last_synced_at),
        page.sync_status ?? 'new',
        page.is_redirect ?? 0,
        toBinding(page.redirect_target),
        toBinding(page.content_model),
        toBinding(page.page_id),
        toBinding(page.revision_id)
      );
      return Number(result.lastInsertRowid);
    }
  }

  /**
   * Update page sync status
   */
  updateSyncStatus(title: string, status: SyncStatus): void {
    const stmt = this.db.prepare(
      `UPDATE pages SET sync_status = ?, updated_at = datetime('now') WHERE title = ?`
    );
    stmt.run(status, title);
  }

  /**
   * Delete a page by title
   */
  deletePage(title: string): boolean {
    const page = this.getPage(title);
    if (!page) {
      return false;
    }

    const tx = this.db.transaction(() => {
      const db = this.db;
      db.prepare('DELETE FROM page_categories WHERE page_id = ?').run(page.id);
      db.prepare('DELETE FROM template_usage WHERE page_id = ?').run(page.id);
      db.prepare('DELETE FROM page_links WHERE source_page_id = ?').run(page.id);
      db.prepare('DELETE FROM page_sections WHERE page_id = ?').run(page.id);
      db.prepare(
        'DELETE FROM template_params WHERE call_id IN (SELECT id FROM template_calls WHERE page_id = ?)'
      ).run(page.id);
      db.prepare('DELETE FROM template_calls WHERE page_id = ?').run(page.id);
      db.prepare('DELETE FROM infobox_kv WHERE page_id = ?').run(page.id);
      db.prepare('DELETE FROM redirects WHERE source_title = ?').run(title);
      db.prepare("DELETE FROM docs_fts WHERE tier = 'content' AND title = ?").run(title);
      db.prepare('DELETE FROM page_sections_fts WHERE title = ?').run(title);
      db.prepare('DELETE FROM module_deps WHERE module_title = ?').run(title);
      if (title.startsWith('Template:')) {
        const templateName = title.replace(/^Template:/i, '');
        db.prepare('DELETE FROM template_metadata WHERE template_name = ?').run(templateName);
      }
      db.prepare('DELETE FROM pages WHERE title = ?').run(title);
    });

    tx();
    return true;
  }

  /**
   * Get page count by status
   */
  getPageCounts(): Record<SyncStatus, number> {
    const stmt = this.db.prepare(
      `SELECT sync_status, COUNT(*) as count FROM pages GROUP BY sync_status`
    );
    const rows = stmt.all() as { sync_status: SyncStatus; count: number }[];
    const counts: Record<SyncStatus, number> = {
      synced: 0,
      local_modified: 0,
      wiki_modified: 0,
      conflict: 0,
      staged: 0,
      new: 0,
    };
    for (const row of rows) {
      counts[row.sync_status] = row.count;
    }
    return counts;
  }

  // =========================================================================
  // Sync log operations
  // =========================================================================

  /**
   * Add an entry to the sync log
   */
  logSync(entry: Omit<SyncLogRecord, 'id' | 'timestamp'>): number {
    const stmt = this.db.prepare(`
      INSERT INTO sync_log (operation, page_title, status, revision_id, error_message, details)
      VALUES (?, ?, ?, ?, ?, ?)
    `);
    const result = stmt.run(
      entry.operation,
      entry.page_title,
      entry.status,
      entry.revision_id,
      entry.error_message,
      entry.details
    );
    return Number(result.lastInsertRowid);
  }

  /**
   * Get recent sync logs
   */
  getSyncLogs(limit: number = 100): SyncLogRecord[] {
    const stmt = this.db.prepare(
      'SELECT * FROM sync_log ORDER BY timestamp DESC LIMIT ?'
    );
    return stmt.all(limit) as SyncLogRecord[];
  }

  // =========================================================================
  // Full-text search operations
  // =========================================================================

  /**
   * Index a page for full-text search
   */
  indexPage(tier: 'content' | 'extension' | 'technical', title: string, content: string): void {
    // First, delete any existing entry
    const deleteStmt = this.db.prepare(
      `DELETE FROM docs_fts WHERE tier = ? AND title = ?`
    );
    deleteStmt.run(tier, title);

    // Insert new entry
    const insertStmt = this.db.prepare(
      `INSERT INTO docs_fts (tier, title, content) VALUES (?, ?, ?)`
    );
    insertStmt.run(tier, title, content);
  }

  /**
   * Search full-text index
   */
  searchFts(query: string, options: { tier?: string; limit?: number } = {}): { tier: string; title: string; snippet: string }[] {
    let sql = `
      SELECT tier, title, snippet(docs_fts, 2, '<mark>', '</mark>', '...', 32) as snippet
      FROM docs_fts
      WHERE docs_fts MATCH ?
    `;
    const params: SqlBinding[] = [query];

    if (options.tier) {
      sql += ' AND tier = ?';
      params.push(options.tier);
    }

    sql += ' ORDER BY rank LIMIT ?';
    params.push(options.limit ?? 50);

    const stmt = this.db.prepare(sql);
    return stmt.all(...params) as { tier: string; title: string; snippet: string }[];
  }

  // =========================================================================
  // Category operations
  // =========================================================================

  /**
   * Get or create a category
   */
  getOrCreateCategory(name: string): number {
    const existing = this.db.prepare(
      'SELECT id FROM categories WHERE name = ?'
    ).get(name) as { id: number } | undefined;

    if (existing) {
      return existing.id;
    }

    const stmt = this.db.prepare(
      'INSERT INTO categories (name) VALUES (?)'
    );
    const result = stmt.run(name);
    return Number(result.lastInsertRowid);
  }

  /**
   * Link a page to a category
   */
  linkPageCategory(pageId: number, categoryName: string): void {
    const categoryId = this.getOrCreateCategory(categoryName);
    const stmt = this.db.prepare(
      'INSERT OR IGNORE INTO page_categories (page_id, category_id) VALUES (?, ?)'
    );
    stmt.run(pageId, categoryId);
  }

  /**
   * Get categories for a page
   */
  getPageCategories(pageId: number): string[] {
    const stmt = this.db.prepare(`
      SELECT c.name FROM categories c
      JOIN page_categories pc ON c.id = pc.category_id
      WHERE pc.page_id = ?
      ORDER BY c.name
    `);
    return (stmt.all(pageId) as { name: string }[]).map(r => r.name);
  }

  // =========================================================================
  // MCP helper methods
  // =========================================================================

  /**
   * Get recently modified pages
   */
  getRecentPages(limit: number = 10): PageRecord[] {
    const stmt = this.db.prepare(
      'SELECT * FROM pages ORDER BY updated_at DESC LIMIT ?'
    );
    return stmt.all(limit) as PageRecord[];
  }

  /**
   * Search (alias for searchFts with consistent interface)
   */
  search(query: string, options: { tier?: string; limit?: number } = {}): { title: string; tier: string; snippet: string }[] {
    try {
      return this.searchFts(query, options);
    } catch {
      // Fall back to simple LIKE search if FTS fails
      let sql = `
        SELECT 'content' as tier, title, substr(content, 1, 200) as snippet
        FROM pages
        WHERE title LIKE ? OR content LIKE ?
        LIMIT ?
      `;
      const pattern = `%${query}%`;
      const stmt = this.db.prepare(sql);
      return stmt.all(pattern, pattern, options.limit ?? 20) as { title: string; tier: string; snippet: string }[];
    }
  }

  /**
   * List pages (alias for getPages with MCP-friendly interface)
   */
  listPages(options: { namespace?: number; syncStatus?: string; limit?: number } = {}): PageRecord[] {
    return this.getPages({
      namespace: options.namespace,
      syncStatus: options.syncStatus as SyncStatus | undefined,
      limit: options.limit,
    });
  }

  /**
   * Get members of a category
   */
  getCategoryMembers(categoryName: string): { title: string; namespace: number }[] {
    // Extract category name without prefix
    const name = categoryName.replace(/^Category:/, '');

    const stmt = this.db.prepare(`
      SELECT p.title, p.namespace FROM pages p
      JOIN page_categories pc ON p.id = pc.page_id
      JOIN categories c ON pc.category_id = c.id
      WHERE c.name = ? OR c.name = ?
      ORDER BY p.title
    `);
    return stmt.all(name, `Category:${name}`) as { title: string; namespace: number }[];
  }

  // =========================================================================
  // Statistics
  // =========================================================================

  /**
   * Get database statistics
   */
  getStats(): {
    totalPages: number;
    byNamespace: Record<number, number>;
    byStatus: Record<SyncStatus, number>;
    byType: Record<string, number>;
    totalCategories: number;
    totalSections: number;
    totalTemplateCalls: number;
    totalTemplateParams: number;
    totalInfoboxEntries: number;
    totalModuleDeps: number;
    totalTemplateMetadata: number;
  } {
    const totalPages = (
      this.db.prepare('SELECT COUNT(*) as count FROM pages').get() as { count: number }
    ).count;

    const byNamespace: Record<number, number> = {};
    const nsRows = this.db
      .prepare('SELECT namespace, COUNT(*) as count FROM pages GROUP BY namespace')
      .all() as { namespace: number; count: number }[];
    for (const row of nsRows) {
      byNamespace[row.namespace] = row.count;
    }

    const byStatus = this.getPageCounts();

    const byType: Record<string, number> = {};
    const typeRows = this.db
      .prepare('SELECT page_type, COUNT(*) as count FROM pages GROUP BY page_type')
      .all() as { page_type: string; count: number }[];
    for (const row of typeRows) {
      byType[row.page_type] = row.count;
    }

    const totalCategories = (
      this.db.prepare('SELECT COUNT(*) as count FROM categories').get() as { count: number }
    ).count;

    const totalSections = (
      this.db.prepare('SELECT COUNT(*) as count FROM page_sections').get() as { count: number }
    ).count;

    const totalTemplateCalls = (
      this.db.prepare('SELECT COUNT(*) as count FROM template_calls').get() as { count: number }
    ).count;

    const totalTemplateParams = (
      this.db.prepare('SELECT COUNT(*) as count FROM template_params').get() as { count: number }
    ).count;

    const totalInfoboxEntries = (
      this.db.prepare('SELECT COUNT(*) as count FROM infobox_kv').get() as { count: number }
    ).count;

    const totalModuleDeps = (
      this.db.prepare('SELECT COUNT(*) as count FROM module_deps').get() as { count: number }
    ).count;

    const totalTemplateMetadata = (
      this.db.prepare('SELECT COUNT(*) as count FROM template_metadata').get() as { count: number }
    ).count;

    return {
      totalPages,
      byNamespace,
      byStatus,
      byType,
      totalCategories,
      totalSections,
      totalTemplateCalls,
      totalTemplateParams,
      totalInfoboxEntries,
      totalModuleDeps,
      totalTemplateMetadata,
    };
  }

  // =========================================================================
  // Transaction support
  // =========================================================================

  /**
   * Run operations in a transaction
   */
  transaction<T>(fn: () => T): T {
    const tx = this.db.transaction(fn);
    return tx();
  }

  // =========================================================================
  // Extension documentation operations (Tier 2)
  // =========================================================================

  /**
   * Insert or update extension doc metadata
   */
  upsertExtensionDoc(doc: {
    extensionName: string;
    sourceWiki?: string;
    version?: string | null;
    pagesCount?: number;
    expiresAt?: string;
  }): number {
    const existing = this.db.prepare(
      'SELECT id FROM extension_docs WHERE extension_name = ?'
    ).get(doc.extensionName) as { id: number } | undefined;

    if (existing) {
      const stmt = this.db.prepare(`
        UPDATE extension_docs SET
          source_wiki = COALESCE(?, source_wiki),
          version = COALESCE(?, version),
          pages_count = COALESCE(?, pages_count),
          fetched_at = datetime('now'),
          expires_at = COALESCE(?, expires_at)
        WHERE extension_name = ?
      `);
      stmt.run(
        toBinding(doc.sourceWiki),
        toBinding(doc.version),
        toBinding(doc.pagesCount),
        toBinding(doc.expiresAt),
        doc.extensionName
      );
      return existing.id;
    } else {
      const stmt = this.db.prepare(`
        INSERT INTO extension_docs (extension_name, source_wiki, version, pages_count, fetched_at, expires_at)
        VALUES (?, ?, ?, ?, datetime('now'), ?)
      `);
      const result = stmt.run(
        doc.extensionName,
        doc.sourceWiki || 'mediawiki.org',
        toBinding(doc.version),
        doc.pagesCount || 0,
        toBinding(doc.expiresAt)
      );
      return Number(result.lastInsertRowid);
    }
  }

  /**
   * Insert or update extension doc page
   */
  upsertExtensionDocPage(page: {
    extensionId: number;
    pageTitle: string;
    localPath: string;
    content: string;
    contentHash: string;
  }): number {
    const existing = this.db.prepare(
      'SELECT id FROM extension_doc_pages WHERE extension_id = ? AND page_title = ?'
    ).get(page.extensionId, page.pageTitle) as { id: number } | undefined;

    if (existing) {
      const stmt = this.db.prepare(`
        UPDATE extension_doc_pages SET
          local_path = ?,
          content = ?,
          content_hash = ?,
          fetched_at = datetime('now')
        WHERE id = ?
      `);
      stmt.run(page.localPath, page.content, page.contentHash, existing.id);
      return existing.id;
    } else {
      const stmt = this.db.prepare(`
        INSERT INTO extension_doc_pages (extension_id, page_title, local_path, content, content_hash, fetched_at)
        VALUES (?, ?, ?, ?, ?, datetime('now'))
      `);
      const result = stmt.run(
        page.extensionId,
        page.pageTitle,
        page.localPath,
        page.content,
        page.contentHash
      );
      return Number(result.lastInsertRowid);
    }
  }

  /**
   * Get all extension docs
   */
  getExtensionDocs(): Array<{
    id: number;
    extensionName: string;
    sourceWiki: string;
    version: string | null;
    pagesCount: number;
    fetchedAt: string;
    expiresAt: string | null;
  }> {
    const stmt = this.db.prepare(`
      SELECT id, extension_name as extensionName, source_wiki as sourceWiki,
             version, pages_count as pagesCount, fetched_at as fetchedAt, expires_at as expiresAt
      FROM extension_docs
      ORDER BY extension_name
    `);
    return stmt.all() as Array<{
      id: number;
      extensionName: string;
      sourceWiki: string;
      version: string | null;
      pagesCount: number;
      fetchedAt: string;
      expiresAt: string | null;
    }>;
  }

  /**
   * Get extension doc by name
   */
  getExtensionDoc(extensionName: string): {
    id: number;
    extensionName: string;
    sourceWiki: string;
    version: string | null;
    pagesCount: number;
    fetchedAt: string;
    expiresAt: string | null;
  } | null {
    const stmt = this.db.prepare(`
      SELECT id, extension_name as extensionName, source_wiki as sourceWiki,
             version, pages_count as pagesCount, fetched_at as fetchedAt, expires_at as expiresAt
      FROM extension_docs
      WHERE extension_name = ?
    `);
    return stmt.get(extensionName) as {
      id: number;
      extensionName: string;
      sourceWiki: string;
      version: string | null;
      pagesCount: number;
      fetchedAt: string;
      expiresAt: string | null;
    } | null;
  }

  /**
   * Get extension doc pages
   */
  getExtensionDocPages(extensionId: number): Array<{
    id: number;
    pageTitle: string;
    localPath: string;
    content: string | null;
    contentHash: string | null;
    fetchedAt: string | null;
  }> {
    const stmt = this.db.prepare(`
      SELECT id, page_title as pageTitle, local_path as localPath,
             content, content_hash as contentHash, fetched_at as fetchedAt
      FROM extension_doc_pages
      WHERE extension_id = ?
      ORDER BY page_title
    `);
    return stmt.all(extensionId) as Array<{
      id: number;
      pageTitle: string;
      localPath: string;
      content: string | null;
      contentHash: string | null;
      fetchedAt: string | null;
    }>;
  }

  /**
   * Delete extension doc and all its pages
   */
  deleteExtensionDoc(extensionName: string): boolean {
    const doc = this.getExtensionDoc(extensionName);
    if (!doc) return false;

    // Delete pages first (foreign key cascade should handle this, but be explicit)
    this.db.prepare(
      'DELETE FROM extension_doc_pages WHERE extension_id = ?'
    ).run(doc.id);

    // Delete from FTS
    this.db.prepare(
      `DELETE FROM docs_fts WHERE tier = 'extension' AND title LIKE ?`
    ).run(`Extension:${extensionName}%`);

    // Delete main record
    const stmt = this.db.prepare(
      'DELETE FROM extension_docs WHERE id = ?'
    );
    const result = stmt.run(doc.id);
    return result.changes > 0;
  }

  // =========================================================================
  // Technical documentation operations (Tier 3)
  // =========================================================================

  /**
   * Insert or update technical doc
   */
  upsertTechnicalDoc(doc: {
    docType: string;
    pageTitle: string;
    localPath: string;
    content: string;
    contentHash: string;
    expiresAt?: string;
  }): number {
    const existing = this.db.prepare(
      'SELECT id FROM technical_docs WHERE doc_type = ? AND page_title = ?'
    ).get(doc.docType, doc.pageTitle) as { id: number } | undefined;

    if (existing) {
      const stmt = this.db.prepare(`
        UPDATE technical_docs SET
          local_path = ?,
          content = ?,
          content_hash = ?,
          fetched_at = datetime('now'),
          expires_at = COALESCE(?, expires_at)
        WHERE id = ?
      `);
      stmt.run(
        doc.localPath,
        toBinding(doc.content),
        toBinding(doc.contentHash),
        toBinding(doc.expiresAt),
        existing.id
      );
      return existing.id;
    } else {
      const stmt = this.db.prepare(`
        INSERT INTO technical_docs (doc_type, page_title, local_path, content, content_hash, fetched_at, expires_at)
        VALUES (?, ?, ?, ?, ?, datetime('now'), ?)
      `);
      const result = stmt.run(
        doc.docType,
        doc.pageTitle,
        doc.localPath,
        toBinding(doc.content),
        toBinding(doc.contentHash),
        toBinding(doc.expiresAt)
      );
      return Number(result.lastInsertRowid);
    }
  }

  /**
   * Get all technical docs
   */
  getTechnicalDocs(docType?: string): Array<{
    id: number;
    docType: string;
    pageTitle: string;
    localPath: string;
    content: string | null;
    contentHash: string | null;
    fetchedAt: string;
    expiresAt: string | null;
  }> {
    let sql = `
      SELECT id, doc_type as docType, page_title as pageTitle, local_path as localPath,
             content, content_hash as contentHash, fetched_at as fetchedAt, expires_at as expiresAt
      FROM technical_docs
    `;
    const params: SqlBinding[] = [];

    if (docType) {
      sql += ' WHERE doc_type = ?';
      params.push(docType);
    }

    sql += ' ORDER BY doc_type, page_title';

    const stmt = this.db.prepare(sql);
    return stmt.all(...params) as Array<{
      id: number;
      docType: string;
      pageTitle: string;
      localPath: string;
      content: string | null;
      contentHash: string | null;
      fetchedAt: string;
      expiresAt: string | null;
    }>;
  }

  /**
   * Get technical doc by type and title
   */
  getTechnicalDoc(docType: string, pageTitle: string): {
    id: number;
    docType: string;
    pageTitle: string;
    localPath: string;
    content: string | null;
    contentHash: string | null;
    fetchedAt: string;
    expiresAt: string | null;
  } | null {
    const stmt = this.db.prepare(`
      SELECT id, doc_type as docType, page_title as pageTitle, local_path as localPath,
             content, content_hash as contentHash, fetched_at as fetchedAt, expires_at as expiresAt
      FROM technical_docs
      WHERE doc_type = ? AND page_title = ?
    `);
    return stmt.get(docType, pageTitle) as {
      id: number;
      docType: string;
      pageTitle: string;
      localPath: string;
      content: string | null;
      contentHash: string | null;
      fetchedAt: string;
      expiresAt: string | null;
    } | null;
  }

  /**
   * Delete technical doc
   */
  deleteTechnicalDoc(docType: string, pageTitle: string): boolean {
    // Delete from FTS
    this.db.prepare(
      `DELETE FROM docs_fts WHERE tier = 'technical' AND title = ?`
    ).run(pageTitle);

    const stmt = this.db.prepare(
      'DELETE FROM technical_docs WHERE doc_type = ? AND page_title = ?'
    );
    const result = stmt.run(docType, pageTitle);
    return result.changes > 0;
  }

  /**
   * Delete all technical docs of a type
   */
  deleteTechnicalDocsByType(docType: string): number {
    // Get titles for FTS cleanup
    const docs = this.getTechnicalDocs(docType);
    for (const doc of docs) {
      this.db.prepare(
        `DELETE FROM docs_fts WHERE tier = 'technical' AND title = ?`
      ).run(doc.pageTitle);
    }

    const stmt = this.db.prepare(
      'DELETE FROM technical_docs WHERE doc_type = ?'
    );
    const result = stmt.run(docType);
    return result.changes;
  }

  /**
   * Get outdated docs (past expiration date)
   */
  getOutdatedDocs(): {
    extensions: Array<{ extensionName: string; expiresAt: string }>;
    technical: Array<{ docType: string; pageTitle: string; expiresAt: string }>;
  } {
    const extStmt = this.db.prepare(`
      SELECT extension_name as extensionName, expires_at as expiresAt
      FROM extension_docs
      WHERE expires_at IS NOT NULL AND expires_at < datetime('now')
    `);

    const techStmt = this.db.prepare(`
      SELECT doc_type as docType, page_title as pageTitle, expires_at as expiresAt
      FROM technical_docs
      WHERE expires_at IS NOT NULL AND expires_at < datetime('now')
    `);

    return {
      extensions: extStmt.all() as Array<{ extensionName: string; expiresAt: string }>,
      technical: techStmt.all() as Array<{ docType: string; pageTitle: string; expiresAt: string }>,
    };
  }

  /**
   * Get documentation statistics
   */
  getDocsStats(): {
    extensionCount: number;
    extensionPagesCount: number;
    technicalCount: number;
    technicalByType: Record<string, number>;
  } {
    const extCount = (this.db.prepare('SELECT COUNT(*) as count FROM extension_docs').get() as { count: number }).count;

    const extPagesCount = (this.db.prepare('SELECT COUNT(*) as count FROM extension_doc_pages').get() as { count: number }).count;

    const techCount = (this.db.prepare('SELECT COUNT(*) as count FROM technical_docs').get() as { count: number }).count;

    const techByType: Record<string, number> = {};
    const typeRows = this.db
      .prepare('SELECT doc_type, COUNT(*) as count FROM technical_docs GROUP BY doc_type')
      .all() as { doc_type: string; count: number }[];
    for (const row of typeRows) {
      techByType[row.doc_type] = row.count;
    }

    return {
      extensionCount: extCount,
      extensionPagesCount: extPagesCount,
      technicalCount: techCount,
      technicalByType: techByType,
    };
  }
}

/**
 * Create and initialize a database instance
 */
export async function createDatabase(dbPath: string): Promise<Database> {
  return Database.create(dbPath);
}
