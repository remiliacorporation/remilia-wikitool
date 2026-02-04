/**
 * Cargo import pipeline (CSV/JSON to page content)
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Namespace, getNamespaceFromTitle } from '../models/namespace.js';
import type { Filesystem } from '../storage/filesystem.js';

export interface ImportSource {
  type: 'csv' | 'json';
  path: string;
}

export interface ImportOptions {
  tableName: string;
  templateName?: string;
  titleField?: string;
  titlePrefix?: string;
  updateMode: 'create' | 'update' | 'upsert';
  categoryName?: string;
  articleHeader?: boolean;
  write?: boolean;
}

export interface ImportError {
  row: number;
  message: string;
  title?: string;
}

export interface ImportPageResult {
  title: string;
  filepath: string;
  action: 'create' | 'update' | 'skip';
  content?: string;
}

export interface ImportResult {
  pagesCreated: string[];
  pagesUpdated: string[];
  pagesSkipped: string[];
  errors: ImportError[];
  pages: ImportPageResult[];
}

export async function importToCargo(
  source: ImportSource,
  options: ImportOptions,
  ctx: { fs: Filesystem }
): Promise<ImportResult> {
  const result: ImportResult = {
    pagesCreated: [],
    pagesUpdated: [],
    pagesSkipped: [],
    errors: [],
    pages: [],
  };

  const absPath = resolve(process.cwd(), source.path);
  const content = readFileSync(absPath, 'utf-8');
  const rows = source.type === 'csv' ? parseCSV(content) : parseJSON(content);

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const title = resolveTitle(row, options);
    if (!title) {
      result.errors.push({ row: i + 1, message: 'Missing title field' });
      continue;
    }

    const filepath = ctx.fs.titleToFilepath(title, false);
    const exists = ctx.fs.fileExists(filepath);

    if (options.updateMode === 'create' && exists) {
      result.pagesSkipped.push(title);
      result.pages.push({ title, filepath, action: 'skip' });
      continue;
    }

    if (options.updateMode === 'update' && !exists) {
      result.pagesSkipped.push(title);
      result.pages.push({ title, filepath, action: 'skip' });
      continue;
    }

    const pageContent = generateCargoPage(row, options, title);
    const action: ImportPageResult['action'] = exists ? 'update' : 'create';

    if (options.write) {
      ctx.fs.writeFile(filepath, pageContent);
    }

    if (action === 'create') {
      result.pagesCreated.push(title);
    } else {
      result.pagesUpdated.push(title);
    }

    result.pages.push({
      title,
      filepath,
      action,
      content: options.write ? undefined : pageContent,
    });
  }

  return result;
}

export function parseCSV(content: string, options: { delimiter?: string } = {}): Record<string, string>[] {
  const delimiter = options.delimiter ?? ',';
  const rows = parseCsvRows(stripBom(content), delimiter);
  if (rows.length === 0) return [];

  const headers = rows[0].map(header => header.trim());
  const records: Record<string, string>[] = [];

  for (let i = 1; i < rows.length; i++) {
    const row = rows[i];
    if (row.every(cell => !cell.trim())) continue;
    const record: Record<string, string> = {};
    for (let j = 0; j < headers.length; j++) {
      const key = headers[j];
      if (!key) continue;
      record[key] = row[j] ?? '';
    }
    records.push(record);
  }

  return records;
}

export function parseJSON(content: string): Record<string, string>[] {
  const trimmed = stripBom(content).trim();
  if (!trimmed) return [];

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return [];
  }

  const rows = Array.isArray(parsed) ? parsed : [];
  const records: Record<string, string>[] = [];

  for (const row of rows) {
    if (!row || typeof row !== 'object') continue;
    const record: Record<string, string> = {};
    for (const [key, value] of Object.entries(row as Record<string, unknown>)) {
      record[key] = value === null || value === undefined ? '' : String(value);
    }
    records.push(record);
  }

  return records;
}

export function generateCargoPage(
  row: Record<string, string>,
  options: ImportOptions,
  title?: string
): string {
  const blocks: string[] = [];
  const namespace = title ? getNamespaceFromTitle(title) : Namespace.Main;

  if (options.articleHeader && namespace === Namespace.Main) {
    const headerLines: string[] = [];
    const shortdesc = pickShortdesc(row, title);
    if (shortdesc) {
      headerLines.push(`{{SHORTDESC:${shortdesc.slice(0, 100)}}}`);
    }
    headerLines.push('{{Article quality|unverified}}');
    blocks.push(headerLines.join('\n'));
  }

  if (options.templateName) {
    const params = Object.entries(row)
      .map(([k, v]) => `|${k}=${escapeCargoValue(v)}`)
      .join('\n');
    blocks.push(`{{${options.templateName}${params ? `\n${params}` : ''}}}`);
  } else {
    const params = Object.entries(row)
      .map(([k, v]) => `|${k}=${escapeCargoValue(v)}`)
      .join('\n');
    blocks.push(`{{#cargo_store:_table=${options.tableName}${params ? `\n${params}` : ''}}}`);
  }

  if (options.categoryName) {
    blocks.push(`[[Category:${options.categoryName}]]`);
  }

  return blocks.join('\n\n');
}

function resolveTitle(row: Record<string, string>, options: ImportOptions): string | null {
  const titleField = options.titleField
    ?? (row.title ? 'title' : row.name ? 'name' : null);
  if (!titleField) return null;

  const raw = row[titleField];
  const base = raw ? raw.trim() : '';
  if (!base) return null;

  const prefix = options.titlePrefix ?? '';
  return prefix ? `${prefix}${base}` : base;
}

function pickShortdesc(row: Record<string, string>, title?: string): string | null {
  const candidates = ['shortdesc', 'description', 'name'];
  for (const key of candidates) {
    const value = row[key];
    if (value && value.trim()) return value.trim();
  }
  if (title && title.trim()) return title.trim();
  return null;
}

function escapeCargoValue(value: string): string {
  if (value.includes('|') || value.includes('}}')) {
    return `<nowiki>${value}</nowiki>`;
  }
  return value;
}

function parseCsvRows(content: string, delimiter: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = '';
  let i = 0;
  let inQuotes = false;

  while (i < content.length) {
    const ch = content[i];

    if (inQuotes) {
      if (ch === '"') {
        if (content[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        inQuotes = false;
        i++;
        continue;
      }
      field += ch;
      i++;
      continue;
    }

    if (ch === '"') {
      inQuotes = true;
      i++;
      continue;
    }

    if (ch === delimiter) {
      row.push(field);
      field = '';
      i++;
      continue;
    }

    if (ch === '\n' || ch === '\r') {
      row.push(field);
      field = '';
      if (ch === '\r' && content[i + 1] === '\n') {
        i++;
      }
      rows.push(row);
      row = [];
      i++;
      continue;
    }

    field += ch;
    i++;
  }

  row.push(field);
  if (row.length > 1 || (row[0] && row[0].trim())) {
    rows.push(row);
  }

  return rows;
}

function stripBom(text: string): string {
  if (text.charCodeAt(0) === 0xfeff) {
    return text.slice(1);
  }
  return text;
}
