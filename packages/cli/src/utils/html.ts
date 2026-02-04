/**
 * Minimal HTML scanning helpers (no regex)
 */

export interface TagMatch {
  name: string;
  attrs: Record<string, string>;
  start: number;
  end: number;
  raw: string;
}

export function extractHead(html: string): string {
  const headStart = findTagStart(html, 'head', 0);
  if (headStart === -1) return html;
  const openEnd = findTagEnd(html, headStart);
  if (openEnd === -1) return html;
  const closeIndex = indexOfIgnoreCase(html, '</head>', openEnd + 1);
  if (closeIndex === -1) return html.slice(openEnd + 1);
  return html.slice(openEnd + 1, closeIndex);
}

export function extractTitle(html: string): string | null {
  const start = findTagStart(html, 'title', 0);
  if (start === -1) return null;
  const openEnd = findTagEnd(html, start);
  if (openEnd === -1) return null;
  const close = indexOfIgnoreCase(html, '</title>', openEnd + 1);
  if (close === -1) return null;
  const raw = html.slice(openEnd + 1, close);
  const decoded = decodeHtml(raw);
  return decoded.trim() || null;
}

export function scanTags(html: string, tagName: string): TagMatch[] {
  const matches: TagMatch[] = [];
  let i = 0;
  const name = tagName.toLowerCase();

  while (i < html.length) {
    const lt = html.indexOf('<', i);
    if (lt === -1) break;
    if (startsWithAt(html, lt, '<!--')) {
      const end = indexOfIgnoreCase(html, '-->', lt + 4);
      i = end === -1 ? html.length : end + 3;
      continue;
    }

    if (isTagAt(html, lt, name)) {
      const end = findTagEnd(html, lt);
      if (end === -1) break;
      const raw = html.slice(lt, end + 1);
      const attrs = parseAttributes(raw, name);
      matches.push({ name, attrs, start: lt, end, raw });
      i = end + 1;
      continue;
    }

    i = lt + 1;
  }

  return matches;
}

export function decodeHtml(text: string): string {
  let decoded = text;
  decoded = replaceAllLiteral(decoded, '&amp;', '&');
  decoded = replaceAllLiteral(decoded, '&quot;', '"');
  decoded = replaceAllLiteral(decoded, '&#39;', "'");
  decoded = replaceAllLiteral(decoded, '&lt;', '<');
  decoded = replaceAllLiteral(decoded, '&gt;', '>');
  return decoded;
}

function findTagStart(html: string, tagName: string, start: number): number {
  let i = start;
  const name = tagName.toLowerCase();
  while (i < html.length) {
    const lt = html.indexOf('<', i);
    if (lt === -1) return -1;
    if (isTagAt(html, lt, name)) return lt;
    i = lt + 1;
  }
  return -1;
}

function isTagAt(html: string, index: number, tagName: string): boolean {
  if (html[index] !== '<') return false;
  let i = index + 1;
  if (i >= html.length) return false;
  if (html[i] === '/') return false;

  for (let j = 0; j < tagName.length; j++) {
    const ch = html[i + j];
    if (!ch || ch.toLowerCase() !== tagName[j]) return false;
  }

  const next = html[i + tagName.length];
  return next === ' ' || next === '\t' || next === '\n' || next === '\r' || next === '>' || next === '/';
}

function findTagEnd(html: string, start: number): number {
  let i = start;
  let quote: string | null = null;

  while (i < html.length) {
    const ch = html[i];
    if (quote) {
      if (ch === quote) {
        quote = null;
      }
      i++;
      continue;
    }
    if (ch === '"' || ch === '\'') {
      quote = ch;
      i++;
      continue;
    }
    if (ch === '>') return i;
    i++;
  }
  return -1;
}

function parseAttributes(tag: string, tagName: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  let i = tagName.length + 1; // after '<tag'

  while (i < tag.length) {
    const ch = tag[i];
    if (ch === '>') break;
    if (isWhitespace(ch) || ch === '/') {
      i++;
      continue;
    }

    const nameStart = i;
    while (i < tag.length && !isWhitespace(tag[i]) && tag[i] !== '=' && tag[i] !== '>' && tag[i] !== '/') {
      i++;
    }
    const rawName = tag.slice(nameStart, i).trim();
    if (!rawName) continue;
    const name = rawName.toLowerCase();

    while (i < tag.length && isWhitespace(tag[i])) i++;
    let value = '';

    if (tag[i] === '=') {
      i++;
      while (i < tag.length && isWhitespace(tag[i])) i++;
      const quote = tag[i] === '"' || tag[i] === '\'' ? tag[i] : null;
      if (quote) {
        i++;
        const valueStart = i;
        while (i < tag.length && tag[i] !== quote) i++;
        value = tag.slice(valueStart, i);
        if (tag[i] === quote) i++;
      } else {
        const valueStart = i;
        while (i < tag.length && !isWhitespace(tag[i]) && tag[i] !== '>') i++;
        value = tag.slice(valueStart, i);
      }
    }

    if (value) {
      attrs[name] = value;
    } else if (!(name in attrs)) {
      attrs[name] = '';
    }
  }

  return attrs;
}

function indexOfIgnoreCase(text: string, search: string, start: number): number {
  const searchLen = search.length;
  if (searchLen === 0) return start;

  for (let i = start; i <= text.length - searchLen; i++) {
    let match = true;
    for (let j = 0; j < searchLen; j++) {
      if (text[i + j].toLowerCase() !== search[j].toLowerCase()) {
        match = false;
        break;
      }
    }
    if (match) return i;
  }
  return -1;
}

function startsWithAt(text: string, index: number, seq: string): boolean {
  if (index + seq.length > text.length) return false;
  for (let i = 0; i < seq.length; i++) {
    if (text[index + i] !== seq[i]) return false;
  }
  return true;
}

function replaceAllLiteral(text: string, search: string, replacement: string): string {
  let out = '';
  let i = 0;
  while (i < text.length) {
    if (text.slice(i, i + search.length) === search) {
      out += replacement;
      i += search.length;
    } else {
      out += text[i];
      i++;
    }
  }
  return out;
}

function isWhitespace(ch: string | undefined): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}
