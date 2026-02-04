/**
 * Minimal Link Parser
 *
 * Extracts wikilinks, categories, and template names from wikitext.
 * Uses a state machine instead of regex for O(n) single-pass parsing
 * that correctly handles nested brackets.
 *
 * Scope: Link extraction only - NOT a full wikitext parser.
 */

import { getNamespaceId, isInterwikiPrefix } from '../config/namespaces.js';
import { parseTemplateCalls } from './context.js';

const METADATA_TEMPLATES = new Set(['SHORTDESC', 'DISPLAYTITLE']);

export interface ParsedLink {
  type: 'internal' | 'interwiki' | 'category';
  target: string;
  displayText?: string;
  namespace?: number;
  raw: string;
}

export interface ParsedContent {
  links: ParsedLink[];
  categories: string[];
  templates: string[];
  redirectTarget?: string;
}

/**
 * Extract wikilinks using character-by-character state machine.
 * Single pass, O(n), handles nested brackets correctly.
 */
export function extractLinks(content: string): string[] {
  const links: string[] = [];
  let i = 0;
  const len = content.length;

  while (i < len - 1) {
    // Look for [[
    if (content[i] === '[' && content[i + 1] === '[') {
      i += 2;
      const start = i;
      let depth = 1;

      // Scan until matching ]]
      while (i < len - 1 && depth > 0) {
        if (content[i] === '[' && content[i + 1] === '[') {
          depth++;
          i += 2;
        } else if (content[i] === ']' && content[i + 1] === ']') {
          depth--;
          if (depth === 0) {
            links.push(content.slice(start, i));
          }
          i += 2;
        } else {
          i++;
        }
      }
    } else {
      i++;
    }
  }

  return links;
}

/**
 * Extract template names using state machine
 */
export function extractTemplates(content: string): string[] {
  const calls = parseTemplateCalls(content);
  const seen = new Set<string>();
  for (const call of calls) {
    if (METADATA_TEMPLATES.has(call.name.toUpperCase())) {
      continue;
    }
    if (!seen.has(call.name)) {
      seen.add(call.name);
    }
  }
  return Array.from(seen);
}

/**
 * Parse extracted link into components
 */
export function parseLink(raw: string): ParsedLink | null {
  let target = raw;
  let displayText: string | undefined;

  // Handle leading colon (forced link)
  const forced = target.startsWith(':');
  if (forced) target = target.slice(1);

  // Strip pipe content: [[Page|display]] → Page
  const pipeIdx = target.indexOf('|');
  if (pipeIdx !== -1) {
    displayText = target.slice(pipeIdx + 1);
    target = target.slice(0, pipeIdx);
  }

  // Strip fragment: [[Page#Section]] → Page
  const hashIdx = target.indexOf('#');
  if (hashIdx !== -1) {
    target = target.slice(0, hashIdx);
  }

  // Normalize
  target = replaceChar(target, '_', ' ').trim();
  if (!target) return null;

  // Check namespace
  const colonIdx = target.indexOf(':');
  if (colonIdx > 0) {
    const prefix = target.slice(0, colonIdx).toLowerCase();
    const rest = target.slice(colonIdx + 1).trim();

    // Skip files entirely (text-only wiki, files handled on wiki itself)
    if (prefix === 'file' || prefix === 'image') return null;

    // Interwiki
    if (isInterwikiPrefix(prefix)) {
      return { type: 'interwiki', target: `${prefix}:${rest}`, displayText, raw };
    }

    // Category
    if (prefix === 'category') {
      return forced
        ? { type: 'internal', target: `Category:${rest}`, displayText, raw }
        : { type: 'category', target: rest, raw };
    }

    // Other namespace
    const nsId = getNamespaceId(prefix);
    if (nsId !== undefined) {
      return { type: 'internal', target, namespace: nsId, displayText, raw };
    }
  }

  // Main namespace - normalize first char to uppercase
  const normalized = target.charAt(0).toUpperCase() + target.slice(1);
  return { type: 'internal', target: normalized, namespace: 0, displayText, raw };
}

function replaceChar(text: string, target: string, replacement: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    out += text[i] === target ? replacement : text[i];
  }
  return out;
}

/**
 * Full content parse using state machine
 */
export function parseContent(content: string): ParsedContent {
  const result: ParsedContent = {
    links: [],
    categories: [],
    templates: [],
  };

  // Check redirect first
  if (content.trimStart().toLowerCase().startsWith('#redirect')) {
    const links = extractLinks(content);
    if (links.length > 0) {
      const parsed = parseLink(links[0]);
      if (parsed) result.redirectTarget = parsed.target;
    }
    return result;
  }

  // Extract and parse links
  for (const raw of extractLinks(content)) {
    const link = parseLink(raw);
    if (!link) continue;

    if (link.type === 'category') {
      result.categories.push(link.target);
    } else {
      result.links.push(link);
    }
  }

  // Extract templates
  result.templates = extractTemplates(content);

  // Dedupe
  result.categories = [...new Set(result.categories)];
  result.templates = [...new Set(result.templates)];

  return result;
}
