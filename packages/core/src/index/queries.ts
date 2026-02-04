/**
 * Index Queries
 *
 * Query functions for link graph, backlinks, and orphan detection.
 */

import type { Database as BunDatabase } from 'bun:sqlite';
import type { Database } from '../storage/sqlite.js';
import type { BacklinkResult } from '../storage/links.js';
import type { CargoColumn, CargoDeclare, CargoStore, CargoQuery } from '../parser/cargo.js';

export interface OrphanResult {
  id: number;
  title: string;
}

export interface TemplateUsageResult {
  pageId: number;
  title: string;
}

export interface CategoryMemberResult {
  pageId: number;
  title: string;
}

export interface SectionResult {
  sectionIndex: number;
  heading: string | null;
  level: number | null;
  anchor: string | null;
  content: string;
  summary: string;
  isLead: boolean;
}

export interface TemplateParamResult {
  paramIndex: number;
  paramName: string | null;
  paramValue: string | null;
  isNamed: boolean;
}

export interface TemplateCallResult {
  callId: number;
  templateName: string;
  callIndex: number;
  params: TemplateParamResult[];
}

export interface InfoboxEntry {
  infoboxName: string;
  paramName: string;
  paramValue: string | null;
  callIndex: number | null;
}

export interface TemplateMetadataResult {
  templateName: string;
  source: string;
  paramDefs: string | null;
  description: string | null;
  example: string | null;
  updatedAt: string;
}

export interface ModuleDependencyResult {
  dependency: string;
  depType: string;
}

export interface CargoTableDeclaration extends CargoDeclare {
  pageTitle: string;
}

export interface CargoStoreEntry extends CargoStore {
  pageTitle: string;
}

export interface CargoQueryEntry extends CargoQuery {
  pageTitle: string;
}

export interface CargoSchemaMismatch {
  pageTitle: string;
  tableName: string;
  field: string;
  message: string;
}

export interface CargoTableContext {
  table: CargoDeclare;
  declaringPage: string;
  stores: { pageTitle: string; values: Record<string, string> }[];
  queries: { pageTitle: string; query: CargoQuery }[];
  fieldUsage: Record<string, { count: number; examples: string[] }>;
}

export interface TemplateParamUsage {
  name: string;
  usageCount: number;
  pageCount: number;
  exampleValues: string[];
}

export interface TemplatePositionalUsage {
  index: number;
  usageCount: number;
  pageCount: number;
  exampleValues: string[];
}

export interface TemplateUsageStats {
  templateName: string;
  totalCalls: number;
  totalPages: number;
  namedParams: TemplateParamUsage[];
  positionalParams: TemplatePositionalUsage[];
  samplePages: TemplateUsageResult[];
}

export interface TemplateSchemaParam {
  name: string;
  required: boolean | null;
  description: string | null;
  type: string | null;
  default: string | null;
  aliases: string[];
  usageCount: number;
  pageCount: number;
  exampleValues: string[];
  source: 'templatedata' | 'observed' | 'merged';
}

export interface TemplateSchema {
  templateName: string;
  source: 'templatedata' | 'observed' | 'merged';
  params: TemplateSchemaParam[];
  positionalParams: TemplatePositionalUsage[];
  notes: string[];
}

export interface ContextBundle {
  title: string;
  namespace: number;
  pageType: string;
  shortdesc: string | null;
  displayTitle: string | null;
  wordCount: number | null;
  content?: string | null;
  sections: SectionResult[];
  categories: string[];
  templates: string[];
  outgoingLinks: Array<{ target: string; type: string }>;
  infobox: InfoboxEntry[];
  templateCalls: TemplateCallResult[];
  templateMetadata?: TemplateMetadataResult | null;
  templateUsage?: TemplateUsageStats;
  templateSchema?: TemplateSchema;
  moduleDependencies?: ModuleDependencyResult[];
  cargoStores?: {
    tableName: string;
    values: Record<string, string>;
  }[];
  cargoSchema?: {
    tableName: string;
    columns: CargoColumn[];
  };
}

export interface TemplateContextBundle {
  templateName: string;
  pageTitle: string;
  page: ContextBundle | null;
  metadata: TemplateMetadataResult | null;
  usage: TemplateUsageStats;
  schema: TemplateSchema;
}

/**
 * Get pages that link to a given title (backlinks)
 */
export function getBacklinks(db: Database, targetTitle: string): BacklinkResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT p.id as pageId, p.title, pl.link_type as linkType
    FROM page_links pl
    JOIN pages p ON pl.source_page_id = p.id
    WHERE pl.target_title = ?
    ORDER BY p.title
  `);

  return stmt.all(targetTitle) as BacklinkResult[];
}

/**
 * Get orphan pages (no incoming links from other pages)
 */
export function getOrphanPages(db: Database): OrphanResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT p.id, p.title
    FROM pages p
    WHERE p.namespace = 0
      AND p.is_redirect = 0
      AND NOT EXISTS (
        SELECT 1 FROM page_links pl WHERE pl.target_title = p.title
      )
      AND NOT EXISTS (
        SELECT 1 FROM redirects r WHERE r.target_title = p.title
      )
    ORDER BY p.title
  `);

  return stmt.all() as OrphanResult[];
}

/**
 * Get pages using a template
 */
export function getPagesUsingTemplate(db: Database, templateName: string): TemplateUsageResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT p.id as pageId, p.title
    FROM template_usage tu
    JOIN pages p ON tu.page_id = p.id
    WHERE tu.template_name = ?
    ORDER BY p.title
  `);

  return stmt.all(templateName) as TemplateUsageResult[];
}

/**
 * Get members of a category (from index, not API)
 */
export function getCategoryMembers(db: Database, categoryName: string): CategoryMemberResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  // Strip Category: prefix if present
  const name = stripPrefixIgnoreCase(categoryName, 'Category:');

  const stmt = rawDb.prepare(`
    SELECT p.id as pageId, p.title
    FROM pages p
    JOIN page_categories pc ON p.id = pc.page_id
    JOIN categories c ON pc.category_id = c.id
    WHERE c.name = ?
    ORDER BY p.title
  `);

  return stmt.all(name) as CategoryMemberResult[];
}

/**
 * Get redirect target for a title
 */
export function getRedirectTarget(db: Database, sourceTitle: string): string | null {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare('SELECT target_title FROM redirects WHERE source_title = ?');
  const row = stmt.get(sourceTitle) as { target_title: string } | undefined;

  return row?.target_title ?? null;
}

/**
 * Resolve a title through redirect chain (max 5 hops)
 */
export function resolveRedirect(db: Database, title: string, maxHops = 5): string {
  let current = title;
  let hops = 0;

  while (hops < maxHops) {
    const target = getRedirectTarget(db, current);
    if (!target || target === current) break;
    current = target;
    hops++;
  }

  return current;
}

/**
 * Get all redirects pointing to a title
 */
export function getRedirectsTo(db: Database, targetTitle: string): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare('SELECT source_title FROM redirects WHERE target_title = ?');
  const rows = stmt.all(targetTitle) as Array<{ source_title: string }>;

  return rows.map(r => r.source_title);
}

/**
 * Get outgoing links from a page
 */
export function getOutgoingLinks(db: Database, pageTitle: string): Array<{ target: string; type: string }> {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT pl.target_title as target, pl.link_type as type
    FROM page_links pl
    JOIN pages p ON pl.source_page_id = p.id
    WHERE p.title = ?
    ORDER BY pl.target_title
  `);

  return stmt.all(pageTitle) as Array<{ target: string; type: string }>;
}

/**
 * Get templates used by a page
 */
export function getPageTemplates(db: Database, pageTitle: string): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT tu.template_name
    FROM template_usage tu
    JOIN pages p ON tu.page_id = p.id
    WHERE p.title = ?
    ORDER BY tu.template_name
  `);

  const rows = stmt.all(pageTitle) as Array<{ template_name: string }>;
  return rows.map(r => r.template_name);
}

/**
 * Get categories of a page (from index)
 */
export function getPageCategories(db: Database, pageTitle: string): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT c.name
    FROM categories c
    JOIN page_categories pc ON c.id = pc.category_id
    JOIN pages p ON pc.page_id = p.id
    WHERE p.title = ?
    ORDER BY c.name
  `);

  const rows = stmt.all(pageTitle) as Array<{ name: string }>;
  return rows.map(r => r.name);
}

// =========================================================================
// Context Queries
// =========================================================================

export function getPageSections(db: Database, pageTitle: string): SectionResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT ps.section_index as sectionIndex,
           ps.heading,
           ps.level,
           ps.anchor,
           ps.content,
           ps.is_lead as isLead
    FROM page_sections ps
    JOIN pages p ON ps.page_id = p.id
    WHERE p.title = ?
    ORDER BY ps.section_index
  `);

  const rows = stmt.all(pageTitle) as Array<{
    sectionIndex: number;
    heading: string | null;
    level: number | null;
    anchor: string | null;
    content: string;
    isLead: number;
  }>;

  return rows.map(row => ({
    ...row,
    summary: summarizeSectionContent(row.content),
    isLead: row.isLead === 1,
  }));
}

export function getTemplateCallsForPage(db: Database, pageTitle: string): TemplateCallResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const callsStmt = rawDb.prepare(`
    SELECT tc.id as callId, tc.template_name as templateName, tc.call_index as callIndex
    FROM template_calls tc
    JOIN pages p ON tc.page_id = p.id
    WHERE p.title = ?
    ORDER BY tc.call_index
  `);

  const calls = callsStmt.all(pageTitle) as Array<{
    callId: number;
    templateName: string;
    callIndex: number;
  }>;

  const paramsStmt = rawDb.prepare(`
    SELECT param_index as paramIndex,
           param_name as paramName,
           param_value as paramValue,
           is_named as isNamed
    FROM template_params
    WHERE call_id = ?
    ORDER BY param_index
  `);

  return calls.map(call => {
    const params = paramsStmt.all(call.callId) as Array<{
      paramIndex: number;
      paramName: string | null;
      paramValue: string | null;
      isNamed: number;
    }>;
    return {
      ...call,
      params: params.map(param => ({
        ...param,
        isNamed: param.isNamed === 1,
      })),
    };
  });
}

export function getInfoboxEntries(db: Database, pageTitle: string): InfoboxEntry[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT infobox_name as infoboxName,
           param_name as paramName,
           param_value as paramValue,
           call_index as callIndex
    FROM infobox_kv
    WHERE page_id = (SELECT id FROM pages WHERE title = ?)
    ORDER BY infobox_name, param_name
  `);

  return stmt.all(pageTitle) as InfoboxEntry[];
}

export function getTemplateMetadata(db: Database, templateName: string): TemplateMetadataResult | null {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT template_name as templateName,
           source,
           param_defs as paramDefs,
           description,
           example,
           updated_at as updatedAt
    FROM template_metadata
    WHERE template_name = ?
  `);

  return (stmt.get(templateName) as TemplateMetadataResult | undefined) ?? null;
}

export function getTemplateUsageStats(
  db: Database,
  templateName: string,
  options: { sampleLimit?: number; valueLimit?: number } = {}
): TemplateUsageStats {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const normalizedName = normalizeTemplateKey(templateName);
  const valueLimit = options.valueLimit ?? 3;
  const sampleLimit = options.sampleLimit ?? 10;

  const calls = rawDb.prepare(`
    SELECT id, page_id as pageId
    FROM template_calls
    WHERE template_name = ?
  `).all(normalizedName) as Array<{ id: number; pageId: number }>;

  const pageIds = new Set<number>();
  for (const call of calls) {
    pageIds.add(call.pageId);
  }

  const paramRows = rawDb.prepare(`
    SELECT tc.page_id as pageId,
           tp.param_index as paramIndex,
           tp.param_name as paramName,
           tp.param_value as paramValue,
           tp.is_named as isNamed
    FROM template_params tp
    JOIN template_calls tc ON tp.call_id = tc.id
    WHERE tc.template_name = ?
  `).all(normalizedName) as Array<{
    pageId: number;
    paramIndex: number;
    paramName: string | null;
    paramValue: string | null;
    isNamed: number;
  }>;

  const namedMap = new Map<string, { usageCount: number; pages: Set<number>; examples: string[] }>();
  const positionalMap = new Map<number, { usageCount: number; pages: Set<number>; examples: string[] }>();

  for (const row of paramRows) {
    const value = row.paramValue ? normalizeParamValue(row.paramValue) : '';
    if (row.isNamed === 1 && row.paramName) {
      const key = row.paramName;
      const entry = namedMap.get(key) ?? { usageCount: 0, pages: new Set<number>(), examples: [] };
      entry.usageCount += 1;
      entry.pages.add(row.pageId);
      if (value && entry.examples.length < valueLimit && !entry.examples.includes(value)) {
        entry.examples.push(value);
      }
      namedMap.set(key, entry);
    } else {
      const key = row.paramIndex;
      const entry = positionalMap.get(key) ?? { usageCount: 0, pages: new Set<number>(), examples: [] };
      entry.usageCount += 1;
      entry.pages.add(row.pageId);
      if (value && entry.examples.length < valueLimit && !entry.examples.includes(value)) {
        entry.examples.push(value);
      }
      positionalMap.set(key, entry);
    }
  }

  const namedParams: TemplateParamUsage[] = Array.from(namedMap.entries()).map(([name, entry]) => ({
    name,
    usageCount: entry.usageCount,
    pageCount: entry.pages.size,
    exampleValues: entry.examples,
  }));

  namedParams.sort((a, b) => b.usageCount - a.usageCount || a.name.localeCompare(b.name));

  const positionalParams: TemplatePositionalUsage[] = Array.from(positionalMap.entries()).map(
    ([index, entry]) => ({
      index,
      usageCount: entry.usageCount,
      pageCount: entry.pages.size,
      exampleValues: entry.examples,
    })
  );

  positionalParams.sort((a, b) => a.index - b.index);

  const samplePages = rawDb.prepare(`
    SELECT p.id as pageId, p.title
    FROM template_usage tu
    JOIN pages p ON tu.page_id = p.id
    WHERE tu.template_name = ?
    ORDER BY p.title
    LIMIT ?
  `).all(normalizedName, sampleLimit) as TemplateUsageResult[];

  return {
    templateName: normalizedName,
    totalCalls: calls.length,
    totalPages: pageIds.size,
    namedParams,
    positionalParams,
    samplePages,
  };
}

export function getTemplateSchema(
  db: Database,
  templateName: string,
  usage?: TemplateUsageStats
): TemplateSchema {
  const normalizedName = normalizeTemplateKey(templateName);
  const templateUsage = usage ?? getTemplateUsageStats(db, normalizedName);
  const metadata = getTemplateMetadata(db, normalizedName);
  const paramDefs = parseTemplateParamDefs(metadata?.paramDefs ?? null);

  const params: TemplateSchemaParam[] = [];
  const notes: string[] = [];
  const names = new Set<string>();
  const usageMap = new Map<string, TemplateParamUsage>();

  if (paramDefs) {
    for (const name of Object.keys(paramDefs)) {
      names.add(name);
    }
  }

  for (const param of templateUsage.namedParams) {
    names.add(param.name);
    usageMap.set(param.name, param);
  }

  for (const name of names) {
    const def = paramDefs ? paramDefs[name] : null;
    const usageEntry = usageMap.get(name);

    const defRequired = def?.required === true ? true : def?.required === false ? false : null;
    const defDescription = typeof def?.description === 'string' ? def.description : null;
    const defType = typeof def?.type === 'string' ? def.type : null;
    const defDefault = typeof def?.default === 'string' ? def.default : null;
    const defAliases = Array.isArray(def?.aliases) ? def.aliases.filter(a => typeof a === 'string') : [];

    const source: TemplateSchemaParam['source'] =
      def && usageEntry ? 'merged' : def ? 'templatedata' : 'observed';

    params.push({
      name,
      required: defRequired,
      description: defDescription,
      type: defType,
      default: defDefault,
      aliases: defAliases,
      usageCount: usageEntry?.usageCount ?? 0,
      pageCount: usageEntry?.pageCount ?? 0,
      exampleValues: usageEntry?.exampleValues ?? [],
      source,
    });
  }

  params.sort((a, b) => a.name.localeCompare(b.name));

  let source: TemplateSchema['source'] = 'observed';
  if (metadata && templateUsage.namedParams.length > 0) source = 'merged';
  else if (metadata) source = 'templatedata';

  if (!metadata) notes.push('TemplateData not available; schema inferred from usage only.');
  if (templateUsage.namedParams.length === 0 && templateUsage.positionalParams.length === 0) {
    notes.push('No template usages found to infer parameter behavior.');
  }

  return {
    templateName: normalizedName,
    source,
    params,
    positionalParams: templateUsage.positionalParams,
    notes,
  };
}

export function getModuleDependencies(db: Database, moduleTitle: string): ModuleDependencyResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT dependency, dep_type as depType
    FROM module_deps
    WHERE module_title = ?
    ORDER BY dep_type, dependency
  `);

  return stmt.all(moduleTitle) as ModuleDependencyResult[];
}

// =========================================================================
// Cargo Queries
// =========================================================================

export function getCargoTableDeclarations(db: Database, tableName: string): CargoTableDeclaration[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT ct.table_name as tableName,
           ct.columns as columns,
           ct.declare_raw as raw,
           p.title as pageTitle
    FROM cargo_tables ct
    JOIN pages p ON ct.page_id = p.id
    WHERE ct.table_name = ?
    ORDER BY p.title
  `);

  const rows = stmt.all(tableName) as Array<{
    tableName: string;
    columns: string;
    raw: string | null;
    pageTitle: string;
  }>;

  return rows.map(row => ({
    type: 'cargo_declare',
    tableName: row.tableName,
    columns: parseJson<CargoColumn[]>(row.columns) ?? [],
    raw: row.raw ?? '',
    pageTitle: row.pageTitle,
  }));
}

export function getCargoTable(db: Database, tableName: string): CargoDeclare | null {
  const declarations = getCargoTableDeclarations(db, tableName);
  if (declarations.length === 0) return null;
  const first = declarations[0];
  return {
    type: 'cargo_declare',
    tableName: first.tableName,
    columns: first.columns,
    raw: first.raw,
  };
}

export function getAllCargoTables(db: Database): CargoDeclare[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT table_name as tableName, columns, declare_raw as raw
    FROM cargo_tables
    ORDER BY table_name
  `);

  const rows = stmt.all() as Array<{ tableName: string; columns: string; raw: string | null }>;
  return rows.map(row => ({
    type: 'cargo_declare',
    tableName: row.tableName,
    columns: parseJson<CargoColumn[]>(row.columns) ?? [],
    raw: row.raw ?? '',
  }));
}

export function getCargoTableColumns(db: Database, tableName: string): CargoColumn[] {
  const table = getCargoTable(db, tableName);
  return table?.columns ?? [];
}

export function getCargoStoresForTable(db: Database, tableName: string): CargoStoreEntry[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT cs.table_name as tableName,
           cs.values_json as valuesJson,
           cs.store_raw as raw,
           p.title as pageTitle
    FROM cargo_stores cs
    JOIN pages p ON cs.page_id = p.id
    WHERE cs.table_name = ?
    ORDER BY p.title
  `);

  const rows = stmt.all(tableName) as Array<{
    tableName: string;
    valuesJson: string;
    raw: string | null;
    pageTitle: string;
  }>;

  return rows.map(row => ({
    type: 'cargo_store',
    tableName: row.tableName,
    values: parseJson<Record<string, string>>(row.valuesJson) ?? {},
    raw: row.raw ?? '',
    pageTitle: row.pageTitle,
  }));
}

export function getCargoQueriesForTable(db: Database, tableName: string): CargoQueryEntry[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT cq.query_type as queryType,
           cq.tables as tables,
           cq.fields as fields,
           cq.params_json as paramsJson,
           cq.query_raw as raw,
           p.title as pageTitle
    FROM cargo_queries cq
    JOIN pages p ON cq.page_id = p.id
    ORDER BY p.title
  `);

  const rows = stmt.all() as Array<{
    queryType: CargoQuery['type'];
    tables: string;
    fields: string | null;
    paramsJson: string;
    raw: string | null;
    pageTitle: string;
  }>;

  const matches: CargoQueryEntry[] = [];
  for (const row of rows) {
    const tables = parseJson<string[]>(row.tables) ?? [];
    if (!tables.includes(tableName)) {
      continue;
    }
    matches.push({
      type: row.queryType,
      tables,
      fields: parseJson<string[]>(row.fields) ?? undefined,
      params: parseJson<Record<string, string>>(row.paramsJson) ?? {},
      raw: row.raw ?? '',
      pageTitle: row.pageTitle,
    });
  }

  return matches;
}

export function getPagesStoringToTable(db: Database, tableName: string): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT DISTINCT p.title as title
    FROM cargo_stores cs
    JOIN pages p ON cs.page_id = p.id
    WHERE cs.table_name = ?
    ORDER BY p.title
  `);

  const rows = stmt.all(tableName) as Array<{ title: string }>;
  return rows.map(row => row.title);
}

export function getOrphanedCargoStores(db: Database): { pageTitle: string; tableName: string }[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const stmt = rawDb.prepare(`
    SELECT p.title as pageTitle, cs.table_name as tableName
    FROM cargo_stores cs
    JOIN pages p ON cs.page_id = p.id
    WHERE cs.table_name NOT IN (SELECT table_name FROM cargo_tables)
    ORDER BY p.title
  `);

  return stmt.all() as Array<{ pageTitle: string; tableName: string }>;
}

export function getDuplicateCargoTables(db: Database): { tableName: string; pageTitles: string[] }[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const rows = rawDb.prepare(`
    SELECT ct.table_name as tableName, p.title as pageTitle
    FROM cargo_tables ct
    JOIN pages p ON ct.page_id = p.id
    ORDER BY ct.table_name, p.title
  `).all() as Array<{ tableName: string; pageTitle: string }>;

  const map = new Map<string, string[]>();
  for (const row of rows) {
    const list = map.get(row.tableName) ?? [];
    list.push(row.pageTitle);
    map.set(row.tableName, list);
  }

  const duplicates: { tableName: string; pageTitles: string[] }[] = [];
  for (const [tableName, pageTitles] of map.entries()) {
    if (pageTitles.length > 1) {
      duplicates.push({ tableName, pageTitles });
    }
  }
  return duplicates;
}

export function getCargoSchemaMismatches(db: Database): CargoSchemaMismatch[] {
  const mismatches: CargoSchemaMismatch[] = [];
  const tables = getAllCargoTables(db);
  const schemaMap = new Map<string, Set<string>>();

  for (const table of tables) {
    const names = new Set<string>();
    for (const column of table.columns) {
      if (column.name) names.add(column.name);
    }
    schemaMap.set(table.tableName, names);
  }

  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const rows = rawDb.prepare(`
    SELECT cs.table_name as tableName, cs.values_json as valuesJson, p.title as pageTitle
    FROM cargo_stores cs
    JOIN pages p ON cs.page_id = p.id
  `).all() as Array<{ tableName: string; valuesJson: string; pageTitle: string }>;

  for (const row of rows) {
    const values = parseJson<Record<string, string>>(row.valuesJson) ?? {};
    const schema = schemaMap.get(row.tableName);
    if (!schema) continue;
    for (const field of Object.keys(values)) {
      if (!schema.has(field)) {
        mismatches.push({
          pageTitle: row.pageTitle,
          tableName: row.tableName,
          field,
          message: `Unknown field "${field}" for table "${row.tableName}"`,
        });
      }
    }
  }

  return mismatches;
}

export function getCargoTableContext(db: Database, tableName: string): CargoTableContext {
  const declarations = getCargoTableDeclarations(db, tableName);
  const primary = declarations[0];
  const table = primary
    ? {
      type: 'cargo_declare' as const,
      tableName: primary.tableName,
      columns: primary.columns,
      raw: primary.raw,
    }
    : {
      type: 'cargo_declare' as const,
      tableName,
      columns: [],
      raw: '',
    };

  const stores = getCargoStoresForTable(db, tableName).map(store => ({
    pageTitle: store.pageTitle,
    values: store.values,
  }));

  const queries = getCargoQueriesForTable(db, tableName).map(query => ({
    pageTitle: query.pageTitle,
    query: {
      type: query.type,
      tables: query.tables,
      fields: query.fields,
      params: query.params,
      raw: query.raw,
    },
  }));

  const fieldUsage: CargoTableContext['fieldUsage'] = {};
  for (const store of stores) {
    for (const [key, value] of Object.entries(store.values)) {
      const entry = fieldUsage[key] ?? { count: 0, examples: [] };
      entry.count += 1;
      if (value && entry.examples.length < 3 && !entry.examples.includes(value)) {
        entry.examples.push(value);
      }
      fieldUsage[key] = entry;
    }
  }

  return {
    table,
    declaringPage: primary?.pageTitle ?? '',
    stores,
    queries,
    fieldUsage,
  };
}

export function getContextBundle(
  db: Database,
  pageTitle: string,
  options: { includeContent?: boolean; maxSections?: number; includeCargo?: boolean; cargoStoreLimit?: number } = {}
): ContextBundle | null {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const pageStmt = rawDb.prepare(`
    SELECT title, namespace, page_type as pageType, shortdesc, display_title as displayTitle,
           word_count as wordCount, content
    FROM pages
    WHERE title = ?
  `);
  const page = pageStmt.get(pageTitle) as {
    title: string;
    namespace: number;
    pageType: string;
    shortdesc: string | null;
    displayTitle: string | null;
    wordCount: number | null;
    content: string | null;
  } | undefined;

  if (!page) return null;

  const sections = getPageSections(db, pageTitle);
  const limitedSections = options.maxSections ? sections.slice(0, options.maxSections) : sections;

  const bundle: ContextBundle = {
    title: page.title,
    namespace: page.namespace,
    pageType: page.pageType,
    shortdesc: page.shortdesc,
    displayTitle: page.displayTitle,
    wordCount: page.wordCount,
    content: options.includeContent ? page.content : undefined,
    sections: limitedSections,
    categories: getPageCategories(db, pageTitle),
    templates: getPageTemplates(db, pageTitle),
    outgoingLinks: getOutgoingLinks(db, pageTitle),
    infobox: getInfoboxEntries(db, pageTitle),
    templateCalls: getTemplateCallsForPage(db, pageTitle),
  };

  if (page.namespace === 10) {
    const templateName = stripPrefixIgnoreCase(pageTitle, 'Template:');
    const normalized = normalizeTemplateKey(templateName);
    bundle.templateMetadata = getTemplateMetadata(db, normalized);
    const usage = getTemplateUsageStats(db, normalized);
    bundle.templateUsage = usage;
    bundle.templateSchema = getTemplateSchema(db, normalized, usage);
  }

  if (page.namespace === 828) {
    bundle.moduleDependencies = getModuleDependencies(db, pageTitle);
  }

  if (options.includeCargo) {
    const storeLimit = options.cargoStoreLimit ?? 25;
    const storeStmt = rawDb.prepare(`
      SELECT cs.table_name as tableName, cs.values_json as valuesJson
      FROM cargo_stores cs
      JOIN pages p ON cs.page_id = p.id
      WHERE p.title = ?
      ORDER BY cs.table_name
      LIMIT ?
    `);

    const storeRows = storeStmt.all(pageTitle, storeLimit) as Array<{
      tableName: string;
      valuesJson: string;
    }>;

    if (storeRows.length > 0) {
      bundle.cargoStores = storeRows.map(row => ({
        tableName: row.tableName,
        values: parseJson<Record<string, string>>(row.valuesJson) ?? {},
      }));
    }

    const schemaStmt = rawDb.prepare(`
      SELECT ct.table_name as tableName, ct.columns as columns
      FROM cargo_tables ct
      JOIN pages p ON ct.page_id = p.id
      WHERE p.title = ?
      ORDER BY ct.table_name
    `);

    const schemaRows = schemaStmt.all(pageTitle) as Array<{ tableName: string; columns: string }>;
    if (schemaRows.length > 0) {
      const first = schemaRows[0];
      bundle.cargoSchema = {
        tableName: first.tableName,
        columns: parseJson<CargoColumn[]>(first.columns) ?? [],
      };
    }
  }

  return bundle;
}

export function getTemplateContextBundle(
  db: Database,
  templateName: string,
  options: { includeContent?: boolean; maxSections?: number; sampleLimit?: number; valueLimit?: number; includeCargo?: boolean; cargoStoreLimit?: number } = {}
): TemplateContextBundle {
  const normalized = normalizeTemplateKey(templateName);
  const pageTitle = `Template:${normalized}`;
  const page = getContextBundle(db, pageTitle, options);
  const metadata = getTemplateMetadata(db, normalized);
  const usage = getTemplateUsageStats(db, normalized, {
    sampleLimit: options.sampleLimit,
    valueLimit: options.valueLimit,
  });
  const schema = getTemplateSchema(db, normalized, usage);

  return {
    templateName: normalized,
    pageTitle,
    page,
    metadata,
    usage,
    schema,
  };
}

// =========================================================================
// Category Cleanup
// =========================================================================

export interface EmptyCategoryResult {
  id: number;
  name: string;
  memberCount: number;
}

export function getEmptyCategories(
  db: Database,
  options: { minMembers?: number } = {}
): EmptyCategoryResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const minMembers = options.minMembers ?? 1;

  const stmt = rawDb.prepare(`
    SELECT c.id, c.name, COUNT(pc.page_id) as memberCount
    FROM categories c
    LEFT JOIN page_categories pc ON c.id = pc.category_id
    GROUP BY c.id
    HAVING memberCount < ?
    ORDER BY c.name
  `);

  return stmt.all(minMembers) as EmptyCategoryResult[];
}

export function pruneEmptyCategories(
  db: Database,
  options: { minMembers?: number; apply?: boolean } = {}
): { removed: number; categories: string[] } {
  const rawDb = (db as unknown as { db: BunDatabase }).db;
  const minMembers = options.minMembers ?? 1;
  const empty = getEmptyCategories(db, { minMembers });
  const names = empty.map(entry => entry.name);

  if (!options.apply || empty.length === 0) {
    return { removed: 0, categories: names };
  }

  const ids = empty.map(entry => entry.id);
  const tx = rawDb.transaction(() => {
    for (const id of ids) {
      rawDb.prepare('DELETE FROM categories WHERE id = ?').run(id);
    }
  });

  tx();
  return { removed: ids.length, categories: names };
}

// =========================================================================
// Validation Queries
// =========================================================================

export interface BrokenLinkResult {
  sourceTitle: string;
  targetTitle: string;
}

export interface DoubleRedirectResult {
  title: string;
  firstTarget: string;
  finalTarget: string;
}

/**
 * Find all broken internal links (links to non-existent pages)
 * Excludes File: and Category: links as those may be valid external resources
 */
export function getBrokenLinks(db: Database): BrokenLinkResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT
      p.title as sourceTitle,
      pl.target_title as targetTitle
    FROM page_links pl
    JOIN pages p ON p.id = pl.source_page_id
    WHERE pl.link_type = 'internal'
    AND pl.target_title NOT IN (SELECT title FROM pages)
    AND pl.target_title NOT LIKE 'File:%'
    AND pl.target_title NOT LIKE 'Category:%'
    ORDER BY p.title
  `);

  return stmt.all() as BrokenLinkResult[];
}

/**
 * Find all double redirects (redirects that point to other redirects)
 */
export function getDoubleRedirects(db: Database): DoubleRedirectResult[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT
      r1.source_title as title,
      r1.target_title as firstTarget,
      r2.target_title as finalTarget
    FROM redirects r1
    JOIN redirects r2 ON r1.target_title = r2.source_title
    ORDER BY r1.source_title
  `);

  return stmt.all() as DoubleRedirectResult[];
}

/**
 * Find uncategorized articles (pages in main namespace with no categories)
 */
export function getUncategorizedPages(db: Database): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT p.title
    FROM pages p
    LEFT JOIN page_categories pc ON p.id = pc.page_id
    WHERE p.namespace = 0
    AND p.is_redirect = 0
    AND pc.page_id IS NULL
    ORDER BY p.title
  `);

  const rows = stmt.all() as Array<{ title: string }>;
  return rows.map(r => r.title);
}

/**
 * Find pages missing SHORTDESC template
 */
export function getMissingShortdesc(db: Database): string[] {
  const rawDb = (db as unknown as { db: BunDatabase }).db;

  const stmt = rawDb.prepare(`
    SELECT p.title
    FROM pages p
    WHERE p.namespace = 0
    AND p.is_redirect = 0
    AND (p.shortdesc IS NULL OR p.shortdesc = '')
    ORDER BY p.title
  `);

  const rows = stmt.all() as Array<{ title: string }>;
  return rows.map(r => r.title);
}

// =========================================================================
// Helpers (no regex)
// =========================================================================

function parseJson<T>(value: string | null): T | null {
  if (!value) return null;
  try {
    return JSON.parse(value) as T;
  } catch {
    return null;
  }
}

function stripPrefixIgnoreCase(text: string, prefix: string): string {
  if (text.length < prefix.length) return text;
  for (let i = 0; i < prefix.length; i++) {
    if (text[i].toLowerCase() !== prefix[i].toLowerCase()) return text;
  }
  return text.slice(prefix.length);
}

function normalizeTemplateKey(name: string): string {
  const trimmed = stripPrefixIgnoreCase(name.trim(), 'Template:');
  const withoutUnderscore = replaceChar(trimmed, '_', ' ');
  const collapsed = collapseWhitespace(withoutUnderscore);
  if (!collapsed) return '';
  return collapsed.charAt(0).toUpperCase() + collapsed.slice(1);
}

function normalizeParamValue(value: string): string {
  const trimmed = value.trim();
  const collapsed = collapseWhitespace(trimmed);
  if (collapsed.length > 120) {
    return collapsed.slice(0, 120);
  }
  return collapsed;
}

function parseTemplateParamDefs(paramDefs: string | null): Record<string, { [key: string]: unknown }> | null {
  if (!paramDefs) return null;
  try {
    const parsed = JSON.parse(paramDefs);
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null;
    return parsed as Record<string, { [key: string]: unknown }>;
  } catch {
    return null;
  }
}

function summarizeSectionContent(content: string): string {
  const plain = extractPlainText(content);
  if (!plain) return '';
  return summarizeText(plain, 2, 240);
}

function extractPlainText(text: string): string {
  let out = '';
  let i = 0;
  let templateDepth = 0;

  while (i < text.length) {
    const ch = text[i];

    if (ch === '<') {
      const commentEnd = skipComment(text, i);
      if (commentEnd !== null) {
        i = commentEnd;
        continue;
      }
      const tagEnd = skipTag(text, i);
      if (tagEnd !== null) {
        i = tagEnd;
        continue;
      }
    }

    if (ch === '{' && text[i + 1] === '{') {
      templateDepth++;
      i += 2;
      continue;
    }

    if (ch === '}' && text[i + 1] === '}') {
      if (templateDepth > 0) templateDepth--;
      i += 2;
      continue;
    }

    if (templateDepth > 0) {
      i++;
      continue;
    }

    if (ch === '[' && text[i + 1] === '[') {
      const link = extractLinkText(text, i + 2);
      if (link) {
        out += link.text;
        i = link.nextIndex;
        continue;
      }
    }

    if (ch === '[') {
      const ext = extractExternalLinkText(text, i + 1);
      if (ext) {
        out += ext.text;
        i = ext.nextIndex;
        continue;
      }
    }

    if (ch === '\'') {
      const end = skipApostrophes(text, i);
      if (end !== null) {
        i = end;
        continue;
      }
    }

    out += ch;
    i++;
  }

  return collapseWhitespace(out).trim();
}

function summarizeText(text: string, maxSentences: number, maxChars: number): string {
  let out = '';
  let sentences = 0;
  let lastBoundary = -1;

  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    out += ch;
    if (ch === '.' || ch === '?' || ch === '!') {
      if (i + 1 >= text.length || isWhitespace(text[i + 1])) {
        sentences++;
        if (sentences >= maxSentences) break;
        lastBoundary = out.length;
      }
    }
    if (out.length >= maxChars) {
      if (lastBoundary > 0) {
        out = out.slice(0, lastBoundary);
      }
      break;
    }
  }

  return out.trim();
}

function extractLinkText(text: string, start: number): { text: string; nextIndex: number } | null {
  let i = start;
  let depth = 1;
  let buffer = '';
  while (i < text.length - 1) {
    if (text[i] === '[' && text[i + 1] === '[') {
      depth++;
      buffer += '[[';
      i += 2;
      continue;
    }
    if (text[i] === ']' && text[i + 1] === ']') {
      depth--;
      if (depth === 0) {
        const display = selectLinkDisplay(buffer);
        return { text: display, nextIndex: i + 2 };
      }
      buffer += ']]';
      i += 2;
      continue;
    }
    buffer += text[i];
    i++;
  }
  return null;
}

function extractExternalLinkText(text: string, start: number): { text: string; nextIndex: number } | null {
  let i = start;
  let buffer = '';
  while (i < text.length) {
    if (text[i] === ']') {
      const display = selectExternalLinkDisplay(buffer);
      return { text: display, nextIndex: i + 1 };
    }
    buffer += text[i];
    i++;
  }
  return null;
}

function selectLinkDisplay(raw: string): string {
  const pipeIndex = raw.lastIndexOf('|');
  if (pipeIndex !== -1) return raw.slice(pipeIndex + 1);
  const hashIndex = raw.indexOf('#');
  const base = hashIndex !== -1 ? raw.slice(0, hashIndex) : raw;
  return base;
}

function selectExternalLinkDisplay(raw: string): string {
  let i = 0;
  while (i < raw.length && raw[i] === ' ') i++;
  const spaceIndex = raw.indexOf(' ', i);
  if (spaceIndex === -1) return '';
  return raw.slice(spaceIndex + 1);
}

function skipComment(text: string, start: number): number | null {
  if (!startsWithAt(text, start, '<!--')) return null;
  const end = indexOfSeq(text, '-->', start + 4);
  return end === -1 ? null : end + 3;
}

function skipTag(text: string, start: number): number | null {
  if (text[start] !== '<') return null;
  let i = start + 1;
  while (i < text.length && text[i] !== '>') i++;
  if (i >= text.length) return null;
  return i + 1;
}

function skipApostrophes(text: string, start: number): number | null {
  let i = start;
  while (i < text.length && text[i] === '\'') i++;
  if (i - start >= 2) return i;
  return null;
}

function collapseWhitespace(text: string): string {
  let out = '';
  let inSpace = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (isWhitespace(ch)) {
      if (!inSpace) {
        out += ' ';
        inSpace = true;
      }
    } else {
      out += ch;
      inSpace = false;
    }
  }
  return out;
}

function replaceChar(text: string, target: string, replacement: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    out += text[i] === target ? replacement : text[i];
  }
  return out;
}

function isWhitespace(ch: string): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}

function startsWithAt(text: string, index: number, seq: string): boolean {
  if (index + seq.length > text.length) return false;
  for (let i = 0; i < seq.length; i++) {
    if (text[index + i] !== seq[i]) return false;
  }
  return true;
}

function indexOfSeq(text: string, seq: string, start: number): number {
  for (let i = start; i <= text.length - seq.length; i++) {
    if (startsWithAt(text, i, seq)) return i;
  }
  return -1;
}
