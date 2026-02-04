/**
 * Index Statistics
 *
 * Statistics and counts for the link index.
 */

import type { Database as BunDatabase } from 'bun:sqlite';
import type { Database } from '../storage/sqlite.js';

export interface IndexStats {
  /** Total number of internal links */
  totalLinks: number;
  /** Total number of interwiki links */
  interwikiLinks: number;
  /** Total number of redirects */
  totalRedirects: number;
  /** Total template usage entries */
  totalTemplateUsages: number;
  /** Number of unique templates used */
  uniqueTemplates: number;
  /** Total category assignments */
  totalCategoryAssignments: number;
  /** Number of categories with members */
  categoriesWithMembers: number;
  /** Number of orphan pages (no incoming links) */
  orphanCount: number;
  /** Total number of indexed sections */
  totalSections: number;
  /** Total number of template calls */
  totalTemplateCalls: number;
  /** Total number of template parameters */
  totalTemplateParams: number;
  /** Total number of infobox key/value entries */
  totalInfoboxEntries: number;
  /** Total number of template metadata entries */
  totalTemplateMetadata: number;
  /** Total number of module dependencies */
  totalModuleDeps: number;
  /** Total number of Cargo table declarations */
  totalCargoTables: number;
  /** Total number of Cargo stores */
  totalCargoStores: number;
  /** Total number of Cargo queries */
  totalCargoQueries: number;
}

export interface TopTemplate {
  name: string;
  usageCount: number;
}

export interface TopCategory {
  name: string;
  memberCount: number;
}

export interface TopLinkedPage {
  title: string;
  incomingLinks: number;
}

/**
 * Get comprehensive index statistics
 */
export function getIndexStats(db: Database): IndexStats {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  // Total internal links
  const internalLinks = (
    rawDb.prepare("SELECT COUNT(*) as count FROM page_links WHERE link_type = 'internal'").get() as { count: number }
  ).count;

  // Interwiki links
  const interwikiLinks = (
    rawDb.prepare("SELECT COUNT(*) as count FROM page_links WHERE link_type = 'interwiki'").get() as { count: number }
  ).count;

  // Total redirects
  const totalRedirects = (
    rawDb.prepare('SELECT COUNT(*) as count FROM redirects').get() as { count: number }
  ).count;

  // Total template usages
  const totalTemplateUsages = (
    rawDb.prepare('SELECT COUNT(*) as count FROM template_usage').get() as { count: number }
  ).count;

  // Unique templates
  const uniqueTemplates = (
    rawDb.prepare('SELECT COUNT(DISTINCT template_name) as count FROM template_usage').get() as { count: number }
  ).count;

  // Total category assignments
  const totalCategoryAssignments = (
    rawDb.prepare('SELECT COUNT(*) as count FROM page_categories').get() as { count: number }
  ).count;

  // Categories with members
  const categoriesWithMembers = (
    rawDb.prepare(`
      SELECT COUNT(DISTINCT category_id) as count FROM page_categories
    `).get() as { count: number }
  ).count;

  // Orphan count (pages with no incoming links)
  const orphanCount = (
    rawDb.prepare(`
      SELECT COUNT(*) as count FROM pages p
      WHERE p.namespace = 0
        AND p.is_redirect = 0
        AND NOT EXISTS (SELECT 1 FROM page_links pl WHERE pl.target_title = p.title)
        AND NOT EXISTS (SELECT 1 FROM redirects r WHERE r.target_title = p.title)
    `).get() as { count: number }
  ).count;

  const totalSections = (
    rawDb.prepare('SELECT COUNT(*) as count FROM page_sections').get() as { count: number }
  ).count;

  const totalTemplateCalls = (
    rawDb.prepare('SELECT COUNT(*) as count FROM template_calls').get() as { count: number }
  ).count;

  const totalTemplateParams = (
    rawDb.prepare('SELECT COUNT(*) as count FROM template_params').get() as { count: number }
  ).count;

  const totalInfoboxEntries = (
    rawDb.prepare('SELECT COUNT(*) as count FROM infobox_kv').get() as { count: number }
  ).count;

  const totalTemplateMetadata = (
    rawDb.prepare('SELECT COUNT(*) as count FROM template_metadata').get() as { count: number }
  ).count;

  const totalModuleDeps = (
    rawDb.prepare('SELECT COUNT(*) as count FROM module_deps').get() as { count: number }
  ).count;

  const totalCargoTables = (
    rawDb.prepare('SELECT COUNT(*) as count FROM cargo_tables').get() as { count: number }
  ).count;

  const totalCargoStores = (
    rawDb.prepare('SELECT COUNT(*) as count FROM cargo_stores').get() as { count: number }
  ).count;

  const totalCargoQueries = (
    rawDb.prepare('SELECT COUNT(*) as count FROM cargo_queries').get() as { count: number }
  ).count;

  return {
    totalLinks: internalLinks,
    interwikiLinks,
    totalRedirects,
    totalTemplateUsages,
    uniqueTemplates,
    totalCategoryAssignments,
    categoriesWithMembers,
    orphanCount,
    totalSections,
    totalTemplateCalls,
    totalTemplateParams,
    totalInfoboxEntries,
    totalTemplateMetadata,
    totalModuleDeps,
    totalCargoTables,
    totalCargoStores,
    totalCargoQueries,
  };
}

/**
 * Get most used templates
 */
export function getTopTemplates(db: Database, limit = 10): TopTemplate[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT template_name as name, COUNT(*) as usageCount
    FROM template_usage
    GROUP BY template_name
    ORDER BY usageCount DESC
    LIMIT ?
  `);

  return stmt.all(limit) as TopTemplate[];
}

/**
 * Get categories with most members
 */
export function getTopCategories(db: Database, limit = 10): TopCategory[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT c.name, COUNT(pc.page_id) as memberCount
    FROM categories c
    JOIN page_categories pc ON c.id = pc.category_id
    GROUP BY c.id
    ORDER BY memberCount DESC
    LIMIT ?
  `);

  return stmt.all(limit) as TopCategory[];
}

/**
 * Get most linked-to pages
 */
export function getTopLinkedPages(db: Database, limit = 10): TopLinkedPage[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT target_title as title, COUNT(*) as incomingLinks
    FROM page_links
    WHERE link_type = 'internal'
    GROUP BY target_title
    ORDER BY incomingLinks DESC
    LIMIT ?
  `);

  return stmt.all(limit) as TopLinkedPage[];
}

/**
 * Check if the index has been built
 */
export function isIndexBuilt(db: Database): boolean {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const linkCount = (
    rawDb.prepare('SELECT COUNT(*) as count FROM page_links').get() as { count: number }
  ).count;

  const redirectCount = (
    rawDb.prepare('SELECT COUNT(*) as count FROM redirects').get() as { count: number }
  ).count;

  // Consider index built if there's any data
  return linkCount > 0 || redirectCount > 0;
}
