/**
 * Index Rebuild
 *
 * Rebuilds all derived indexes (links, categories, templates, redirects)
 * from page content. Runs in a single transaction for atomicity.
 */

import type { Database as BunDatabase, SQLQueryBindings } from 'bun:sqlite';
import type { Database } from '../storage/sqlite.js';
import { parseContent } from '../parser/links.js';
import {
  parseSections,
  parseTemplateCalls,
  parseTemplateData,
  parseModuleDependencies,
} from '../parser/context.js';
import { parseCargo } from '../parser/cargo.js';
import { extractMetadata } from '../parser/metadata.js';

export interface RebuildResult {
  pagesProcessed: number;
  linksStored: number;
  categoriesStored: number;
  templatesStored: number;
  redirectsMapped: number;
  metadataUpdated: number;
  errors: string[];
}

export interface RebuildOptions {
  /** Only rebuild for specific namespace (default: all) */
  namespace?: number;
  /** Emit progress events */
  onProgress?: (processed: number, total: number) => void;
}

/**
 * Rebuild all derived indexes from page content
 * Runs in a single transaction for atomicity
 */
export function rebuildIndex(db: Database, options: RebuildOptions = {}): RebuildResult {
  const result: RebuildResult = {
    pagesProcessed: 0,
    linksStored: 0,
    categoriesStored: 0,
    templatesStored: 0,
    redirectsMapped: 0,
    metadataUpdated: 0,
    errors: [],
  };

  // Get internal db handle
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const transaction = rawDb.transaction(() => {
    // Clear derived tables
    rawDb.run('DELETE FROM page_links');
    rawDb.run('DELETE FROM template_usage');
    rawDb.run('DELETE FROM redirects');
    rawDb.run('DELETE FROM page_categories');
    rawDb.run('DELETE FROM page_sections');
    rawDb.run('DELETE FROM page_sections_fts');
    rawDb.run('DELETE FROM template_calls');
    rawDb.run('DELETE FROM template_params');
    rawDb.run('DELETE FROM infobox_kv');
    rawDb.run('DELETE FROM template_metadata');
    rawDb.run('DELETE FROM module_deps');
    rawDb.run('DELETE FROM cargo_tables');
    rawDb.run('DELETE FROM cargo_stores');
    rawDb.run('DELETE FROM cargo_queries');

    // Build query based on options
    let pageQuery = 'SELECT id, title, content, is_redirect, namespace FROM pages WHERE content IS NOT NULL';
    const params: SQLQueryBindings[] = [];

    if (options.namespace !== undefined) {
      pageQuery += ' AND namespace = ?';
      params.push(options.namespace);
    }

    // Get pages to process
    const pages = rawDb.prepare(pageQuery).all(...params) as Array<{
      id: number;
      title: string;
      content: string;
      is_redirect: number;
      namespace: number;
    }>;

    const totalPages = pages.length;

    // Prepare statements
    const insertLink = rawDb.prepare(
      'INSERT OR IGNORE INTO page_links (source_page_id, target_title, link_type, target_namespace) VALUES (?, ?, ?, ?)'
    );

    const insertTemplate = rawDb.prepare(
      'INSERT INTO template_usage (page_id, template_name) VALUES (?, ?)'
    );

    const insertRedirect = rawDb.prepare(
      'INSERT OR REPLACE INTO redirects (source_title, target_title) VALUES (?, ?)'
    );

    const upsertCategory = rawDb.prepare(
      'INSERT OR IGNORE INTO categories (name) VALUES (?)'
    );

    const getCategoryId = rawDb.prepare(
      'SELECT id FROM categories WHERE name = ?'
    );

    const insertPageCategory = rawDb.prepare(
      'INSERT OR IGNORE INTO page_categories (page_id, category_id) VALUES (?, ?)'
    );

    const updateMetadata = rawDb.prepare(
      'UPDATE pages SET shortdesc = ?, display_title = ?, word_count = ? WHERE id = ?'
    );

    const insertSection = rawDb.prepare(
      `INSERT INTO page_sections (page_id, section_index, heading, level, anchor, content, is_lead)
       VALUES (?, ?, ?, ?, ?, ?, ?)`
    );

    const insertSectionFts = rawDb.prepare(
      `INSERT INTO page_sections_fts (title, heading, content, page_id, section_index)
       VALUES (?, ?, ?, ?, ?)`
    );

    const insertTemplateCall = rawDb.prepare(
      `INSERT INTO template_calls (page_id, template_name, call_index)
       VALUES (?, ?, ?)`
    );

    const insertTemplateParam = rawDb.prepare(
      `INSERT INTO template_params (call_id, param_index, param_name, param_value, is_named)
       VALUES (?, ?, ?, ?, ?)`
    );

    const insertInfobox = rawDb.prepare(
      `INSERT INTO infobox_kv (page_id, infobox_name, param_name, param_value, call_index)
       VALUES (?, ?, ?, ?, ?)`
    );

    const upsertTemplateMetadata = rawDb.prepare(
      `INSERT INTO template_metadata (template_name, source, param_defs, description, example, updated_at)
       VALUES (?, ?, ?, ?, ?, datetime('now'))
       ON CONFLICT(template_name) DO UPDATE SET
         source = excluded.source,
         param_defs = excluded.param_defs,
         description = excluded.description,
         example = excluded.example,
         updated_at = excluded.updated_at`
    );

    const insertModuleDep = rawDb.prepare(
      `INSERT INTO module_deps (module_title, dependency, dep_type)
       VALUES (?, ?, ?)`
    );

    const insertCargoTable = rawDb.prepare(
      `INSERT OR REPLACE INTO cargo_tables (page_id, table_name, columns, declare_raw)
       VALUES (?, ?, ?, ?)`
    );

    const insertCargoStore = rawDb.prepare(
      `INSERT INTO cargo_stores (page_id, table_name, values_json, store_raw)
       VALUES (?, ?, ?, ?)`
    );

    const insertCargoQuery = rawDb.prepare(
      `INSERT INTO cargo_queries (page_id, query_type, tables, fields, params_json, query_raw)
       VALUES (?, ?, ?, ?, ?, ?)`
    );

    for (const page of pages) {
      try {
        const parsed = parseContent(page.content);

        // Handle redirects
        if (parsed.redirectTarget) {
          insertRedirect.run(page.title, parsed.redirectTarget);
          result.redirectsMapped++;
          result.pagesProcessed++;

          // Report progress
          if (options.onProgress) {
            options.onProgress(result.pagesProcessed, totalPages);
          }
          continue;
        }

        // Store links
        for (const link of parsed.links) {
          if (link.type === 'internal') {
            insertLink.run(page.id, link.target, link.type, link.namespace ?? 0);
            result.linksStored++;
          } else if (link.type === 'interwiki') {
            insertLink.run(page.id, link.target, link.type, null);
            result.linksStored++;
          }
        }

        // Store categories (existing tables)
        for (const category of parsed.categories) {
          upsertCategory.run(category);
          const row = getCategoryId.get(category) as { id: number } | undefined;
          if (row) {
            insertPageCategory.run(page.id, row.id);
            result.categoriesStored++;
          }
        }

        // Store template usage
        for (const template of parsed.templates) {
          insertTemplate.run(page.id, template);
          result.templatesStored++;
        }

        // Extract and store metadata
        const metadata = extractMetadata(page.content);
        updateMetadata.run(
          metadata.shortdesc ?? null,
          metadata.displayTitle ?? null,
          metadata.wordCount,
          page.id
        );
        result.metadataUpdated++;

        // Sections (lead + headings)
        const sections = parseSections(page.content);
        for (const section of sections) {
          insertSection.run(
            page.id,
            section.index,
            section.heading,
            section.level,
            section.anchor,
            section.content,
            section.isLead ? 1 : 0
          );
          insertSectionFts.run(
            page.title,
            section.heading ?? '',
            section.content,
            page.id,
            section.index
          );
        }

        // Template calls + params + infoboxes
        const templateCalls = parseTemplateCalls(page.content);
        let callIndex = 0;
        for (const call of templateCalls) {
          callIndex += 1;
          const callResult = insertTemplateCall.run(page.id, call.name, callIndex);
          const callId = Number(callResult.lastInsertRowid);

          for (const param of call.params) {
            insertTemplateParam.run(
              callId,
              param.index,
              param.name,
              param.value,
              param.isNamed ? 1 : 0
            );
          }

          if (isInfoboxName(call.name)) {
            for (const param of call.params) {
              if (param.isNamed && param.name) {
                insertInfobox.run(
                  page.id,
                  call.name,
                  param.name,
                  param.value,
                  callIndex
                );
              }
            }
          }
        }

        // Template metadata (TemplateData)
        if (page.namespace === 10) {
          const templateData = parseTemplateData(page.content);
          if (templateData) {
            const templateName = stripTemplatePrefix(page.title).trim();
            upsertTemplateMetadata.run(
              templateName,
              templateData.source,
              templateData.paramDefs,
              templateData.description,
              templateData.example
            );
          }
        }

        // Module dependencies
        if (page.namespace === 828) {
          const deps = parseModuleDependencies(page.content);
          for (const dep of deps) {
            insertModuleDep.run(page.title, dep.dependency, dep.type);
          }
        }

        // Cargo constructs
        const cargoConstructs = parseCargo(page.content);
        for (const construct of cargoConstructs) {
          if (construct.type === 'cargo_declare') {
            insertCargoTable.run(
              page.id,
              construct.tableName,
              JSON.stringify(construct.columns),
              construct.raw
            );
          } else if (construct.type === 'cargo_store') {
            insertCargoStore.run(
              page.id,
              construct.tableName,
              JSON.stringify(construct.values),
              construct.raw
            );
          } else {
            insertCargoQuery.run(
              page.id,
              construct.type,
              JSON.stringify(construct.tables),
              construct.fields ? JSON.stringify(construct.fields) : null,
              JSON.stringify(construct.params),
              construct.raw
            );
          }
        }

        result.pagesProcessed++;

        // Report progress
        if (options.onProgress) {
          options.onProgress(result.pagesProcessed, totalPages);
        }
      } catch (error) {
        result.errors.push(
          `Error processing "${page.title}": ${error instanceof Error ? error.message : String(error)}`
        );
      }
    }
  });

  try {
    transaction();
  } catch (error) {
    result.errors.push(
      `Transaction failed: ${error instanceof Error ? error.message : String(error)}`
    );
  }

  return result;
}

function isInfoboxName(name: string): boolean {
  if (name.length < 7) return false;
  const prefix = name.slice(0, 7).toLowerCase();
  if (prefix !== 'infobox') return false;
  if (name.length === 7) return true;
  const next = name[7];
  return next === ' ' || next === '_';
}

function stripTemplatePrefix(title: string): string {
  const prefix = 'template:';
  if (title.length < prefix.length) return title;
  for (let i = 0; i < prefix.length; i++) {
    if (title[i].toLowerCase() !== prefix[i]) return title;
  }
  return title.slice(prefix.length);
}
