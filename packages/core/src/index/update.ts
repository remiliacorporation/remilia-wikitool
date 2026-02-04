/**
 * Incremental index updates
 *
 * Updates derived index tables for a single page after pull/push edits.
 */

import type { Database as BunDatabase } from 'bun:sqlite';
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

export interface PageIndexInput {
  id: number;
  title: string;
  namespace: number;
  content: string;
}

export interface UpdatePageIndexResult {
  linksStored: number;
  categoriesStored: number;
  templatesStored: number;
  sectionsStored: number;
  templateCallsStored: number;
  templateParamsStored: number;
  infoboxEntriesStored: number;
  moduleDepsStored: number;
  metadataUpdated: boolean;
  redirectMapped: boolean;
}

export function updatePageIndex(db: Database, page: PageIndexInput): UpdatePageIndexResult {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const result: UpdatePageIndexResult = {
    linksStored: 0,
    categoriesStored: 0,
    templatesStored: 0,
    sectionsStored: 0,
    templateCallsStored: 0,
    templateParamsStored: 0,
    infoboxEntriesStored: 0,
    moduleDepsStored: 0,
    metadataUpdated: false,
    redirectMapped: false,
  };

  const transaction = rawDb.transaction(() => {
    // Clear existing derived data for this page
    rawDb.prepare('DELETE FROM page_links WHERE source_page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM template_usage WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM page_categories WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM page_sections WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM page_sections_fts WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM infobox_kv WHERE page_id = ?').run(page.id);
    rawDb.prepare(
      'DELETE FROM template_params WHERE call_id IN (SELECT id FROM template_calls WHERE page_id = ?)'
    ).run(page.id);
    rawDb.prepare('DELETE FROM template_calls WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM redirects WHERE source_title = ?').run(page.title);
    rawDb.prepare('DELETE FROM module_deps WHERE module_title = ?').run(page.title);
    rawDb.prepare('DELETE FROM cargo_tables WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM cargo_stores WHERE page_id = ?').run(page.id);
    rawDb.prepare('DELETE FROM cargo_queries WHERE page_id = ?').run(page.id);

    // Clear template metadata if this is a template page
    if (page.namespace === 10 || page.title.toLowerCase().startsWith('template:')) {
      const templateName = stripTemplatePrefix(page.title).trim();
      rawDb.prepare('DELETE FROM template_metadata WHERE template_name = ?').run(templateName);
    }

    const parsed = parseContent(page.content);

    if (parsed.redirectTarget) {
      rawDb.prepare(
        'INSERT OR REPLACE INTO redirects (source_title, target_title) VALUES (?, ?)'
      ).run(page.title, parsed.redirectTarget);
      result.redirectMapped = true;
      return;
    }

    // Store links
    if (parsed.links.length > 0) {
      const insertLink = rawDb.prepare(
        'INSERT OR IGNORE INTO page_links (source_page_id, target_title, link_type, target_namespace) VALUES (?, ?, ?, ?)'
      );
      for (const link of parsed.links) {
        insertLink.run(page.id, link.target, link.type, link.namespace ?? null);
        result.linksStored++;
      }
    }

    // Store categories
    if (parsed.categories.length > 0) {
      const upsertCategory = rawDb.prepare(
        'INSERT OR IGNORE INTO categories (name) VALUES (?)'
      );
      const getCategoryId = rawDb.prepare(
        'SELECT id FROM categories WHERE name = ?'
      );
      const insertPageCategory = rawDb.prepare(
        'INSERT OR IGNORE INTO page_categories (page_id, category_id) VALUES (?, ?)'
      );

      for (const category of parsed.categories) {
        upsertCategory.run(category);
        const row = getCategoryId.get(category) as { id: number } | undefined;
        if (row) {
          insertPageCategory.run(page.id, row.id);
          result.categoriesStored++;
        }
      }
    }

    // Store template usage
    if (parsed.templates.length > 0) {
      const insertTemplate = rawDb.prepare(
        'INSERT INTO template_usage (page_id, template_name) VALUES (?, ?)'
      );
      for (const template of parsed.templates) {
        insertTemplate.run(page.id, template);
        result.templatesStored++;
      }
    }

    // Extract and store metadata
    const metadata = extractMetadata(page.content);
    rawDb.prepare(
      'UPDATE pages SET shortdesc = ?, display_title = ?, word_count = ? WHERE id = ?'
    ).run(
      metadata.shortdesc ?? null,
      metadata.displayTitle ?? null,
      metadata.wordCount,
      page.id
    );
    result.metadataUpdated = true;

    // Sections
    const sections = parseSections(page.content);
    if (sections.length > 0) {
      const insertSection = rawDb.prepare(
        `INSERT INTO page_sections (page_id, section_index, heading, level, anchor, content, is_lead)
         VALUES (?, ?, ?, ?, ?, ?, ?)`
      );
      const insertSectionFts = rawDb.prepare(
        `INSERT INTO page_sections_fts (title, heading, content, page_id, section_index)
         VALUES (?, ?, ?, ?, ?)`
      );
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
        result.sectionsStored++;
      }
    }

    // Template calls + params + infoboxes
    const templateCalls = parseTemplateCalls(page.content);
    if (templateCalls.length > 0) {
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

      let callIndex = 0;
      for (const call of templateCalls) {
        callIndex += 1;
        const callResult = insertTemplateCall.run(page.id, call.name, callIndex);
        const callId = Number(callResult.lastInsertRowid);
        result.templateCallsStored++;

        for (const param of call.params) {
          insertTemplateParam.run(
            callId,
            param.index,
            param.name,
            param.value,
            param.isNamed ? 1 : 0
          );
          result.templateParamsStored++;
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
              result.infoboxEntriesStored++;
            }
          }
        }
      }
    }

    // Template metadata (TemplateData)
    if (page.namespace === 10) {
      const templateData = parseTemplateData(page.content);
      if (templateData) {
        const templateName = stripTemplatePrefix(page.title).trim();
        rawDb.prepare(
          `INSERT INTO template_metadata (template_name, source, param_defs, description, example, updated_at)
           VALUES (?, ?, ?, ?, ?, datetime('now'))
           ON CONFLICT(template_name) DO UPDATE SET
             source = excluded.source,
             param_defs = excluded.param_defs,
             description = excluded.description,
             example = excluded.example,
             updated_at = excluded.updated_at`
        ).run(
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
      if (deps.length > 0) {
        const insertModuleDep = rawDb.prepare(
          'INSERT INTO module_deps (module_title, dependency, dep_type) VALUES (?, ?, ?)'
        );
        for (const dep of deps) {
          insertModuleDep.run(page.title, dep.dependency, dep.type);
          result.moduleDepsStored++;
        }
      }
    }

    // Cargo constructs
    const cargoConstructs = parseCargo(page.content);
    if (cargoConstructs.length > 0) {
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
    }
  });

  transaction();
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
