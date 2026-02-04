/**
 * Cargo parser
 *
 * Parses Cargo parser functions (#cargo_*) into structured constructs.
 */

import { parseParserFunctions, type ParserFunctionCall, type TemplateParam } from './context.js';

export interface CargoDeclare {
  type: 'cargo_declare';
  tableName: string;
  columns: CargoColumn[];
  raw: string;
}

export interface CargoColumn {
  name: string;
  type: CargoFieldType | 'Unknown';
  typeRaw: string;
  isList?: boolean;
  delimiter?: string;
  allowedValues?: string[];
}

export type CargoFieldType =
  | 'Text' | 'Wikitext' | 'Wikitext string' | 'Page'
  | 'Integer' | 'Float' | 'Rating' | 'Date' | 'Datetime'
  | 'Boolean' | 'Coordinates' | 'File' | 'URL' | 'Email'
  | 'Searchtext';

export interface CargoStore {
  type: 'cargo_store';
  tableName: string;
  values: Record<string, string>;
  raw: string;
}

export interface CargoQuery {
  type: 'cargo_query' | 'cargo_compound_query';
  tables: string[];
  fields?: string[];
  params: Record<string, string>;
  raw: string;
}

export type CargoConstruct = CargoDeclare | CargoStore | CargoQuery;

const KNOWN_TYPES: CargoFieldType[] = [
  'Text',
  'Wikitext',
  'Wikitext string',
  'Page',
  'Integer',
  'Float',
  'Rating',
  'Date',
  'Datetime',
  'Boolean',
  'Coordinates',
  'File',
  'URL',
  'Email',
  'Searchtext',
];

export function parseCargo(content: string): CargoConstruct[] {
  const constructs: CargoConstruct[] = [];
  const calls = parseParserFunctions(content);

  for (const call of calls) {
    if (call.name === 'cargo_declare') {
      const tableName = getTableName(call);
      if (!tableName) continue;
      const columns = parseCargoColumns(call.params);
      constructs.push({
        type: 'cargo_declare',
        tableName,
        columns,
        raw: call.raw,
      });
      continue;
    }

    if (call.name === 'cargo_store') {
      const tableName = getTableName(call);
      if (!tableName) continue;
      const values = parseCargoValues(call.params);
      constructs.push({
        type: 'cargo_store',
        tableName,
        values,
        raw: call.raw,
      });
      continue;
    }

    if (call.name === 'cargo_query' || call.name === 'cargo_compound_query') {
      const params = collectParams(call.params);
      const tables = parseList(params.tables || params.table || params._table || '');
      const fields = parseList(params.fields || params.field || '');
      constructs.push({
        type: call.name,
        tables,
        fields: fields.length > 0 ? fields : undefined,
        params,
        raw: call.raw,
      });
    }
  }

  return constructs;
}

export function parseCargoColumnType(typeStr: string): CargoColumn {
  const typeRaw = typeStr.trim();
  if (!typeRaw) {
    return { name: '', type: 'Unknown', typeRaw: '' };
  }

  const normalized = collapseWhitespace(typeRaw);
  const lower = normalized.toLowerCase();

  let baseType = normalized;
  let isList = false;
  let delimiter: string | undefined;

  if (startsWithWord(lower, 'list')) {
    isList = true;
    const parsed = parseListType(normalized);
    baseType = parsed.baseType;
    delimiter = parsed.delimiter;
  }

  const allowedValues = parseAllowedValues(normalized);
  const matchedType = matchKnownType(baseType);

  return {
    name: '',
    type: matchedType ?? 'Unknown',
    typeRaw: typeRaw,
    isList: isList || undefined,
    delimiter,
    allowedValues,
  };
}

function parseCargoColumns(params: TemplateParam[]): CargoColumn[] {
  const columns: CargoColumn[] = [];
  for (const param of params) {
    const name = param.name?.trim();
    if (!name || name === '_table' || name === 'table') {
      continue;
    }
    const column = parseCargoColumnType(param.value);
    column.name = name;
    columns.push(column);
  }
  return columns;
}

function parseCargoValues(params: TemplateParam[]): Record<string, string> {
  const values: Record<string, string> = {};
  for (const param of params) {
    const name = param.name?.trim();
    if (!name || name === '_table' || name === 'table') {
      continue;
    }
    values[name] = param.value;
  }
  return values;
}

function getTableName(call: ParserFunctionCall): string | null {
  for (const param of call.params) {
    if (!param.name) continue;
    const key = param.name.trim().toLowerCase();
    if (key === '_table' || key === 'table') {
      const value = param.value.trim();
      return value || null;
    }
  }
  return null;
}

function collectParams(params: TemplateParam[]): Record<string, string> {
  const map: Record<string, string> = {};
  for (const param of params) {
    if (param.name) {
      map[param.name.trim()] = param.value.trim();
    }
  }
  return map;
}

function parseList(value: string): string[] {
  const trimmed = value.trim();
  if (!trimmed) return [];
  return splitByDelimiters(trimmed, [',', ';']);
}

function parseListType(input: string): { baseType: string; delimiter?: string } {
  const lower = input.toLowerCase();
  let i = 4; // after "list"
  while (i < lower.length && isWhitespace(lower[i])) i++;
  let delimiter: string | undefined;

  if (lower[i] === '(') {
    const end = findChar(lower, ')', i + 1);
    if (end !== -1) {
      const rawDelim = input.slice(i + 1, end).trim();
      if (rawDelim) delimiter = rawDelim;
      i = end + 1;
    }
  }

  while (i < lower.length && isWhitespace(lower[i])) i++;
  if (lower.slice(i, i + 2) === 'of') {
    i += 2;
  }
  while (i < lower.length && isWhitespace(lower[i])) i++;

  const baseType = input.slice(i).trim() || input.trim();
  return { baseType, delimiter };
}

function parseAllowedValues(input: string): string[] | undefined {
  const lower = input.toLowerCase();
  const marker = 'allowed values';
  const index = lower.indexOf(marker);
  if (index === -1) return undefined;
  const eqIndex = findChar(lower, '=', index + marker.length);
  if (eqIndex === -1) return undefined;
  const raw = input.slice(eqIndex + 1).trim();
  if (!raw) return undefined;
  const values = splitByDelimiters(raw, [',', ';']);
  return values.length > 0 ? values : undefined;
}

function matchKnownType(typeName: string): CargoFieldType | null {
  const normalized = normalizeTypeKey(typeName);
  for (const known of KNOWN_TYPES) {
    if (normalizeTypeKey(known) === normalized) {
      return known;
    }
  }
  return null;
}

function normalizeTypeKey(input: string): string {
  return collapseWhitespace(input).toLowerCase();
}

function startsWithWord(input: string, word: string): boolean {
  if (!input.startsWith(word)) return false;
  if (input.length === word.length) return true;
  return isWhitespace(input[word.length]);
}

function splitByDelimiters(input: string, delimiters: string[]): string[] {
  const out: string[] = [];
  let current = '';

  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (delimiters.includes(ch)) {
      const trimmed = current.trim();
      if (trimmed) out.push(trimmed);
      current = '';
      continue;
    }
    current += ch;
  }

  const trimmed = current.trim();
  if (trimmed) out.push(trimmed);
  return out;
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
  return out.trim();
}

function isWhitespace(ch: string | undefined): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}

function findChar(text: string, target: string, start: number): number {
  for (let i = start; i < text.length; i++) {
    if (text[i] === target) return i;
  }
  return -1;
}
