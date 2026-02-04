/**
 * Metadata Extraction
 *
 * Extracts page metadata from wikitext:
 * - SHORTDESC: Short description for search results
 * - DISPLAYTITLE: Custom display title
 * - Word count: Visible prose word count
 */

import { calculateWordCount } from './wordcount.js';
import { parseTemplateCalls } from './context.js';

export interface PageMetadata {
  /** Short description from {{SHORTDESC:...}} */
  shortdesc?: string;
  /** Display title from {{DISPLAYTITLE:...}} */
  displayTitle?: string;
  /** Word count of visible prose */
  wordCount: number;
}

const SHORTDESC_NAME = 'SHORTDESC';
const DISPLAYTITLE_NAME = 'DISPLAYTITLE';

/**
 * Extract SHORTDESC from content
 */
export function extractShortdesc(content: string): string | undefined {
  const call = findTemplateCall(content, SHORTDESC_NAME);
  if (!call) return undefined;
  const value = firstParamValue(call);
  return value ? cleanInlineText(value) : undefined;
}

/**
 * Extract DISPLAYTITLE from content
 */
export function extractDisplayTitle(content: string): string | undefined {
  const call = findTemplateCall(content, DISPLAYTITLE_NAME);
  if (!call) return undefined;
  const value = firstParamValue(call);
  return value ? cleanInlineText(value) : undefined;
}

/**
 * Extract all page metadata from content
 */
export function extractMetadata(content: string): PageMetadata {
  return {
    shortdesc: extractShortdesc(content),
    displayTitle: extractDisplayTitle(content),
    wordCount: calculateWordCount(content),
  };
}

/**
 * Check if a page has a short description
 */
export function hasShortdesc(content: string): boolean {
  return extractShortdesc(content) !== undefined;
}

/**
 * Check if a page has a custom display title
 */
export function hasDisplayTitle(content: string): boolean {
  return extractDisplayTitle(content) !== undefined;
}

function findTemplateCall(content: string, name: string) {
  const calls = parseTemplateCalls(content);
  for (const call of calls) {
    if (call.name.toUpperCase() === name) return call;
  }
  return null;
}

function firstParamValue(call: { params: Array<{ value: string }> }): string | null {
  if (call.params.length === 0) return null;
  const value = call.params[0].value.trim();
  return value ? value : null;
}

function cleanInlineText(text: string): string {
  let result = '';
  let i = 0;
  while (i < text.length) {
    const ch = text[i];

    if (ch === '{' && text[i + 1] === '{') {
      const end = findMatchingBraces(text, i);
      if (end !== null) {
        i = end;
        continue;
      }
    }

    if (ch === '[' && text[i + 1] === '[') {
      const link = extractLinkText(text, i + 2);
      if (link) {
        result += link.text;
        i = link.nextIndex;
        continue;
      }
    }

    if (ch === '[') {
      const ext = extractExternalLinkText(text, i + 1);
      if (ext) {
        result += ext.text;
        i = ext.nextIndex;
        continue;
      }
    }

    if (ch === '<') {
      const end = skipTag(text, i);
      if (end !== null) {
        i = end;
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

    result += ch;
    i++;
  }

  return collapseWhitespace(result).trim();
}

function findMatchingBraces(text: string, start: number): number | null {
  if (text[start] !== '{' || text[start + 1] !== '{') return null;
  let i = start + 2;
  let depth = 1;
  while (i < text.length - 1) {
    if (text[i] === '{' && text[i + 1] === '{') {
      depth++;
      i += 2;
      continue;
    }
    if (text[i] === '}' && text[i + 1] === '}') {
      depth--;
      i += 2;
      if (depth === 0) return i;
      continue;
    }
    i++;
  }
  return null;
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
  if (pipeIndex !== -1) {
    return raw.slice(pipeIndex + 1);
  }
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

function isWhitespace(ch: string): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}