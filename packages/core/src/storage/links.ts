/**
 * Link Storage Operations
 *
 * Store and query link graph data: page links, template usage, redirects.
 * Uses the page_links, template_usage, and redirects tables from SCHEMA_003.
 */

import type { Database as BunDatabase } from 'bun:sqlite';

export interface StoredLink {
  id: number;
  sourcePageId: number;
  targetTitle: string;
  linkType: string;
  targetNamespace: number | null;
}

export interface StoredRedirect {
  sourceTitle: string;
  targetTitle: string;
}

export interface BacklinkResult {
  pageId: number;
  title: string;
  linkType: string;
}

/**
 * Link storage operations class
 */
export class LinkStorage {
  constructor(private db: BunDatabase) {}

  /**
   * Clear all links for a page
   */
  clearPageLinks(pageId: number): void {
    this.db.prepare('DELETE FROM page_links WHERE source_page_id = ?').run(pageId);
  }

  /**
   * Insert a link
   */
  insertLink(pageId: number, targetTitle: string, linkType: string, targetNamespace?: number): void {
    this.db
      .prepare(
        `INSERT OR IGNORE INTO page_links (source_page_id, target_title, link_type, target_namespace)
         VALUES (?, ?, ?, ?)`
      )
      .run(pageId, targetTitle, linkType, targetNamespace ?? null);
  }

  /**
   * Bulk insert links for a page
   */
  setPageLinks(
    pageId: number,
    links: Array<{ target: string; type: string; namespace?: number }>
  ): void {
    const insertStmt = this.db.prepare(
      `INSERT OR IGNORE INTO page_links (source_page_id, target_title, link_type, target_namespace)
       VALUES (?, ?, ?, ?)`
    );

    this.db.transaction(() => {
      this.clearPageLinks(pageId);
      for (const link of links) {
        insertStmt.run(pageId, link.target, link.type, link.namespace ?? null);
      }
    })();
  }

  /**
   * Get all links from a page
   */
  getPageLinks(pageId: number): StoredLink[] {
    return this.db
      .prepare(
        `SELECT id, source_page_id as sourcePageId, target_title as targetTitle,
                link_type as linkType, target_namespace as targetNamespace
         FROM page_links WHERE source_page_id = ?`
      )
      .all(pageId) as StoredLink[];
  }

  /**
   * Get backlinks (pages that link to this title)
   */
  getBacklinks(targetTitle: string): BacklinkResult[] {
    return this.db
      .prepare(
        `SELECT p.id as pageId, p.title, pl.link_type as linkType
         FROM page_links pl
         JOIN pages p ON pl.source_page_id = p.id
         WHERE pl.target_title = ?
         ORDER BY p.title`
      )
      .all(targetTitle) as BacklinkResult[];
  }

  /**
   * Get orphan pages (no incoming links)
   */
  getOrphanPages(): Array<{ id: number; title: string }> {
    return this.db
      .prepare(
        `SELECT p.id, p.title
         FROM pages p
         WHERE p.namespace = 0
           AND p.is_redirect = 0
           AND NOT EXISTS (
             SELECT 1 FROM page_links pl WHERE pl.target_title = p.title
           )
           AND NOT EXISTS (
             SELECT 1 FROM redirects r WHERE r.target_title = p.title
           )
         ORDER BY p.title`
      )
      .all() as Array<{ id: number; title: string }>;
  }

  // --- Template usage ---

  /**
   * Clear template usage for a page
   */
  clearTemplateUsage(pageId: number): void {
    this.db.prepare('DELETE FROM template_usage WHERE page_id = ?').run(pageId);
  }

  /**
   * Set templates used by a page
   */
  setTemplateUsage(pageId: number, templates: string[]): void {
    const insertStmt = this.db.prepare(
      'INSERT INTO template_usage (page_id, template_name) VALUES (?, ?)'
    );

    this.db.transaction(() => {
      this.clearTemplateUsage(pageId);
      for (const template of templates) {
        insertStmt.run(pageId, template);
      }
    })();
  }

  /**
   * Get pages using a template
   */
  getPagesUsingTemplate(templateName: string): Array<{ id: number; title: string }> {
    return this.db
      .prepare(
        `SELECT p.id, p.title
         FROM template_usage tu
         JOIN pages p ON tu.page_id = p.id
         WHERE tu.template_name = ?
         ORDER BY p.title`
      )
      .all(templateName) as Array<{ id: number; title: string }>;
  }

  // --- Redirects ---

  /**
   * Set a redirect mapping
   */
  setRedirect(sourceTitle: string, targetTitle: string): void {
    this.db
      .prepare('INSERT OR REPLACE INTO redirects (source_title, target_title) VALUES (?, ?)')
      .run(sourceTitle, targetTitle);
  }

  /**
   * Get redirect target
   */
  getRedirectTarget(sourceTitle: string): string | null {
    const row = this.db
      .prepare('SELECT target_title FROM redirects WHERE source_title = ?')
      .get(sourceTitle) as { target_title: string } | undefined;
    return row?.target_title ?? null;
  }

  /**
   * Get all redirects to a page
   */
  getRedirectsTo(targetTitle: string): string[] {
    const rows = this.db
      .prepare('SELECT source_title FROM redirects WHERE target_title = ?')
      .all(targetTitle) as Array<{ source_title: string }>;
    return rows.map(r => r.source_title);
  }

  /**
   * Resolve a title through redirect chain (max 5 hops)
   */
  resolveRedirect(title: string, maxHops = 5): string {
    let current = title;
    let hops = 0;

    while (hops < maxHops) {
      const target = this.getRedirectTarget(current);
      if (!target || target === current) break;
      current = target;
      hops++;
    }

    return current;
  }

  // --- Statistics ---

  /**
   * Get link graph statistics
   */
  getStats(): {
    totalLinks: number;
    totalRedirects: number;
    totalTemplateUsages: number;
    orphanCount: number;
  } {
    const linksRow = this.db
      .prepare('SELECT COUNT(*) as count FROM page_links')
      .get() as { count: number };

    const redirectsRow = this.db
      .prepare('SELECT COUNT(*) as count FROM redirects')
      .get() as { count: number };

    const templatesRow = this.db
      .prepare('SELECT COUNT(*) as count FROM template_usage')
      .get() as { count: number };

    const orphans = this.getOrphanPages();

    return {
      totalLinks: linksRow.count,
      totalRedirects: redirectsRow.count,
      totalTemplateUsages: templatesRow.count,
      orphanCount: orphans.length,
    };
  }
}
