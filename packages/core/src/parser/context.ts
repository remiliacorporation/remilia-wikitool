/**
 * Context layer parsers
 *
 * Deterministic parsing for sections, template calls/params, template metadata,
 * and module dependencies. These are intentionally conservative and avoid
 * full wikitext parsing.
 */

export interface ParsedSection {
  index: number;
  heading: string | null;
  level: number;
  anchor: string | null;
  content: string;
  isLead: boolean;
}

export interface TemplateParam {
  index: number;
  name: string | null;
  value: string;
  isNamed: boolean;
}

export interface TemplateCall {
  name: string;
  rawName: string;
  params: TemplateParam[];
  raw: string;
}

export interface ParserFunctionCall {
  name: string;
  rawName: string;
  params: TemplateParam[];
  raw: string;
}

export interface TemplateMetadata {
  source: 'templatedata';
  paramDefs: string | null;
  description: string | null;
  example: string | null;
}

export interface ModuleDependency {
  dependency: string;
  type: 'require' | 'mw.loadData' | 'mw.loadJsonData';
}

const MAGIC_WORDS = new Set(['SHORTDESC', 'DISPLAYTITLE']);
const SKIP_TAGS = new Set(['nowiki', 'pre', 'code', 'syntaxhighlight']);

export function parseSections(content: string): ParsedSection[] {
  const lines = normalizeNewlines(content).split('\n');
  const sections: ParsedSection[] = [];

  let currentLines: string[] = [];
  let currentHeading: string | null = null;
  let currentLevel = 0;
  let currentIsLead = true;
  let sectionIndex = 0;

  const flush = () => {
    const text = currentLines.join('\n').trimEnd();
    if (currentIsLead || currentHeading || text.length > 0) {
      sections.push({
        index: sectionIndex,
        heading: currentHeading,
        level: currentLevel,
        anchor: currentHeading ? normalizeAnchor(currentHeading) : null,
        content: text,
        isLead: currentIsLead,
      });
    }
  };

  for (const line of lines) {
    const heading = parseHeadingLine(line);
    if (heading) {
      flush();
      sectionIndex += 1;
      currentHeading = heading.heading;
      currentLevel = heading.level;
      currentIsLead = false;
      currentLines = [];
    } else {
      currentLines.push(line);
    }
  }

  flush();
  return sections;
}

export function parseTemplateCalls(content: string): TemplateCall[] {
  const calls: TemplateCall[] = [];
  for (const raw of parseTransclusions(content)) {
    const call = parseTemplateCallRaw(raw);
    if (call) calls.push(call);
  }
  return calls;
}

export function parseParserFunctions(content: string): ParserFunctionCall[] {
  const calls: ParserFunctionCall[] = [];
  for (const raw of parseTransclusions(content)) {
    const call = parseParserFunctionRaw(raw);
    if (call) calls.push(call);
  }
  return calls;
}

export function parseTemplateData(content: string): TemplateMetadata | null {
  const raw = extractTagContent(content, 'templatedata');
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as {
      params?: Record<string, unknown>;
      description?: string;
      example?: string;
    };
    return {
      source: 'templatedata',
      paramDefs: parsed.params ? JSON.stringify(parsed.params) : null,
      description: typeof parsed.description === 'string' ? parsed.description : null,
      example: typeof parsed.example === 'string' ? parsed.example : null,
    };
  } catch {
    return {
      source: 'templatedata',
      paramDefs: raw,
      description: null,
      example: null,
    };
  }
}

export function parseModuleDependencies(content: string): ModuleDependency[] {
  const deps: ModuleDependency[] = [];
  const seen = new Set<string>();

  const len = content.length;
  let i = 0;

  while (i < len) {
    const ch = content[i];

    // Comments
    if (ch === '-' && content[i + 1] === '-') {
      if (content[i + 2] === '[' && content[i + 3] === '[') {
        const end = findLongBracketEnd(content, i + 2);
        i = end ?? len;
      } else {
        i = skipLine(content, i + 2);
      }
      continue;
    }

    // Strings
    if (ch === '\'' || ch === '"') {
      const end = skipString(content, i, ch);
      i = end ?? len;
      continue;
    }

    // Long strings
    if (ch === '[' && content[i + 1] === '[') {
      const end = findLongBracketEnd(content, i);
      if (end !== null) {
        i = end;
        continue;
      }
    }

    if (isIdentStart(ch)) {
      const start = i;
      i++;
      while (i < len && isIdentChar(content[i])) i++;
      const ident = content.slice(start, i);

      if (ident === 'require') {
        const dep = readStringArgument(content, i);
        if (dep) {
          const normalized = normalizeModuleTitle(dep.value);
          const key = `require:${normalized}`;
          if (!seen.has(key)) {
            seen.add(key);
            deps.push({ dependency: normalized, type: 'require' });
          }
          i = dep.nextIndex;
        }
        continue;
      }

      if (ident === 'mw') {
        const afterMw = skipWhitespace(content, i);
        if (content[afterMw] === '.') {
          const afterDot = skipWhitespace(content, afterMw + 1);
          const nameStart = afterDot;
          let j = nameStart;
          while (j < len && isIdentChar(content[j])) j++;
          const method = content.slice(nameStart, j);
          if (method === 'loadData' || method === 'loadJsonData') {
            const dep = readStringArgument(content, j);
            if (dep) {
              const normalized = normalizeModuleTitle(dep.value);
              const type = method === 'loadData' ? 'mw.loadData' : 'mw.loadJsonData';
              const key = `${type}:${normalized}`;
              if (!seen.has(key)) {
                seen.add(key);
                deps.push({ dependency: normalized, type });
              }
              i = dep.nextIndex;
              continue;
            }
          }
        }
      }
    }

    i++;
  }

  return deps;
}

function parseTransclusions(content: string): string[] {
  const calls: string[] = [];
  const len = content.length;
  let i = 0;

  while (i < len - 1) {
    if (content[i] === '<') {
      const commentSkip = skipComment(content, i);
      if (commentSkip !== null) {
        i = commentSkip;
        continue;
      }
      const tagSkip = skipTag(content, i);
      if (tagSkip !== null) {
        i = tagSkip;
        continue;
      }
    }

    if (content[i] === '{' && content[i + 1] === '{') {
      if (content[i + 2] === '{') {
        const end = findMatchingTripleBrace(content, i);
        if (end === null) break;
        i = end;
        continue;
      }

      const start = i + 2;
      i += 2;
      let depth = 1;
      let paramDepth = 0;

      while (i < len - 1 && depth > 0) {
        if (content[i] === '<') {
          const commentSkip = skipComment(content, i);
          if (commentSkip !== null) {
            i = commentSkip;
            continue;
          }
          const tagSkip = skipTag(content, i);
          if (tagSkip !== null) {
            i = tagSkip;
            continue;
          }
        }

        if (content[i] === '{' && content[i + 1] === '{') {
          if (content[i + 2] === '{') {
            paramDepth++;
            i += 3;
            continue;
          }
          depth++;
          i += 2;
          continue;
        }

        if (content[i] === '}' && content[i + 1] === '}') {
          if (paramDepth > 0 && content[i + 2] === '}') {
            paramDepth--;
            i += 3;
            continue;
          }
          depth--;
          if (depth === 0) {
            calls.push(content.slice(start, i));
            i += 2;
            break;
          }
          i += 2;
          continue;
        }

        i++;
      }

      continue;
    }

    i++;
  }

  return calls;
}

function parseTemplateCallRaw(raw: string): TemplateCall | null {
  const parts = splitTopLevel(raw, '|');
  if (parts.length === 0) return null;

  const rawName = parts[0].trim();
  if (!rawName) return null;
  if (rawName.startsWith('#') || rawName.startsWith(':')) return null;

  let name = rawName;
  let leadingParam: string | null = null;

  const colonIndex = rawName.indexOf(':');
  if (colonIndex > 0) {
    const prefix = rawName.slice(0, colonIndex).trim();
    const rest = rawName.slice(colonIndex + 1).trim();
    if (equalsIgnoreCase(prefix, 'template')) {
      name = rest;
    } else if (MAGIC_WORDS.has(prefix.toUpperCase())) {
      name = prefix;
      if (rest) {
        leadingParam = rest;
      }
    }
  }

  name = normalizeTemplateName(name);
  if (!name) return null;

  const params: TemplateParam[] = [];
  let paramIndexOffset = 1;

  if (leadingParam !== null) {
    const parsed = parseTemplateParam(leadingParam, 1);
    if (parsed) params.push(parsed);
    paramIndexOffset = 2;
  }

  for (let i = 1; i < parts.length; i++) {
    const part = parts[i];
    const parsed = parseTemplateParam(part, i + paramIndexOffset - 1);
    if (parsed) params.push(parsed);
  }

  return { name, rawName, params, raw };
}

function parseParserFunctionRaw(raw: string): ParserFunctionCall | null {
  const parts = splitTopLevel(raw, '|');
  if (parts.length === 0) return null;

  const rawName = parts[0].trim();
  if (!rawName || rawName[0] !== '#') return null;

  let namePart = rawName;
  let leadingParam: string | null = null;

  const colonIndex = rawName.indexOf(':');
  if (colonIndex > 1) {
    namePart = rawName.slice(0, colonIndex).trim();
    const rest = rawName.slice(colonIndex + 1).trim();
    if (rest) {
      leadingParam = rest;
    }
  }

  let name = namePart.slice(1).trim();
  if (!name) return null;
  name = collapseWhitespace(name).toLowerCase();

  const params: TemplateParam[] = [];
  let paramIndexOffset = 1;

  if (leadingParam !== null) {
    params.push({
      index: 1,
      name: null,
      value: leadingParam.trim(),
      isNamed: false,
    });
    paramIndexOffset = 2;
  }

  for (let i = 1; i < parts.length; i++) {
    const part = parts[i];
    const parsed = parseTemplateParam(part, i + paramIndexOffset - 1);
    if (parsed) params.push(parsed);
  }

  return { name, rawName, params, raw };
}

function parseTemplateParam(raw: string, index: number): TemplateParam | null {
  const trimmed = raw.trim();
  if (!trimmed) {
    return { index, name: null, value: '', isNamed: false };
  }

  const eqIndex = findTopLevelChar(trimmed, '=');
  if (eqIndex !== -1) {
    const name = trimmed.slice(0, eqIndex).trim();
    const value = trimmed.slice(eqIndex + 1).trim();
    if (!name) {
      return { index, name: null, value: trimmed, isNamed: false };
    }
    return { index, name, value, isNamed: true };
  }

  return { index, name: null, value: trimmed, isNamed: false };
}

function normalizeTemplateName(name: string): string {
  let cleaned = replaceChar(name.trim(), '_', ' ');
  cleaned = collapseWhitespace(cleaned);
  if (!cleaned) return '';
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}

function normalizeModuleTitle(raw: string): string {
  let cleaned = replaceChar(raw.trim(), '_', ' ');
  cleaned = collapseWhitespace(cleaned);
  if (!cleaned) return '';
  if (!hasColon(cleaned)) {
    cleaned = `Module:${cleaned}`;
  }
  const colonIndex = cleaned.indexOf(':');
  if (colonIndex > 0 && colonIndex < cleaned.length - 1) {
    const prefix = cleaned.slice(0, colonIndex);
    const rest = cleaned.slice(colonIndex + 1);
    return `${prefix}:${rest.charAt(0).toUpperCase()}${rest.slice(1)}`;
  }
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}

function normalizeAnchor(text: string): string {
  const collapsed = collapseWhitespace(text.trim());
  return replaceChar(collapsed, ' ', '_');
}

function parseHeadingLine(line: string): { heading: string; level: number } | null {
  if (!line.startsWith('=')) return null;
  const len = line.length;
  let i = 0;
  while (i < len && line[i] === '=') i++;
  const level = i;
  if (level < 2 || level > 6) return null;

  let end = len - 1;
  while (end >= 0 && isWhitespace(line[end])) end--;
  let trailing = 0;
  while (end >= 0 && line[end] === '=') {
    trailing++;
    end--;
  }
  if (trailing !== level) return null;

  const inner = line.slice(level, end + 1).trim();
  if (!inner) return null;
  return { heading: inner, level };
}

function normalizeNewlines(text: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '\r') {
      if (text[i + 1] === '\n') {
        i++;
      }
      out += '\n';
    } else {
      out += ch;
    }
  }
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

function replaceChar(text: string, target: string, replacement: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    out += text[i] === target ? replacement : text[i];
  }
  return out;
}

function hasColon(text: string): boolean {
  return text.indexOf(':') !== -1;
}

function isWhitespace(ch: string | undefined): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}

function equalsIgnoreCase(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].toLowerCase() !== b[i].toLowerCase()) return false;
  }
  return true;
}

function splitTopLevel(input: string, delimiter: string): string[] {
  const parts: string[] = [];
  let current = '';
  let i = 0;
  let depth = 0;
  let paramDepth = 0;
  let linkDepth = 0;

  while (i < input.length) {
    if (input[i] === '<') {
      const commentSkip = skipComment(input, i);
      if (commentSkip !== null) {
        current += input.slice(i, commentSkip);
        i = commentSkip;
        continue;
      }
      const tagSkip = skipTag(input, i);
      if (tagSkip !== null) {
        current += input.slice(i, tagSkip);
        i = tagSkip;
        continue;
      }
    }

    if (input[i] === '[' && input[i + 1] === '[') {
      linkDepth++;
      current += '[[';
      i += 2;
      continue;
    }
    if (input[i] === ']' && input[i + 1] === ']' && linkDepth > 0) {
      linkDepth--;
      current += ']]';
      i += 2;
      continue;
    }

    if (input[i] === '{' && input[i + 1] === '{') {
      if (input[i + 2] === '{') {
        paramDepth++;
        current += '{{{';
        i += 3;
        continue;
      }
      depth++;
      current += '{{';
      i += 2;
      continue;
    }

    if (input[i] === '}' && input[i + 1] === '}') {
      if (paramDepth > 0 && input[i + 2] === '}') {
        paramDepth--;
        current += '}}}';
        i += 3;
        continue;
      }
      if (depth > 0) {
        depth--;
      }
      current += '}}';
      i += 2;
      continue;
    }

    if (
      input[i] === delimiter &&
      depth === 0 &&
      paramDepth === 0 &&
      linkDepth === 0
    ) {
      parts.push(current);
      current = '';
      i++;
      continue;
    }

    current += input[i];
    i++;
  }

  parts.push(current);
  return parts;
}

function findTopLevelChar(input: string, char: string): number {
  let i = 0;
  let depth = 0;
  let paramDepth = 0;
  let linkDepth = 0;

  while (i < input.length) {
    if (input[i] === '<') {
      const commentSkip = skipComment(input, i);
      if (commentSkip !== null) {
        i = commentSkip;
        continue;
      }
      const tagSkip = skipTag(input, i);
      if (tagSkip !== null) {
        i = tagSkip;
        continue;
      }
    }

    if (input[i] === '[' && input[i + 1] === '[') {
      linkDepth++;
      i += 2;
      continue;
    }
    if (input[i] === ']' && input[i + 1] === ']' && linkDepth > 0) {
      linkDepth--;
      i += 2;
      continue;
    }

    if (input[i] === '{' && input[i + 1] === '{') {
      if (input[i + 2] === '{') {
        paramDepth++;
        i += 3;
        continue;
      }
      depth++;
      i += 2;
      continue;
    }

    if (input[i] === '}' && input[i + 1] === '}') {
      if (paramDepth > 0 && input[i + 2] === '}') {
        paramDepth--;
        i += 3;
        continue;
      }
      if (depth > 0) {
        depth--;
      }
      i += 2;
      continue;
    }

    if (input[i] === char && depth === 0 && paramDepth === 0 && linkDepth === 0) {
      return i;
    }

    i++;
  }

  return -1;
}

function findMatchingTripleBrace(content: string, start: number): number | null {
  let i = start + 3;
  let depth = 1;
  while (i < content.length - 2) {
    if (content[i] === '{' && content[i + 1] === '{' && content[i + 2] === '{') {
      depth++;
      i += 3;
      continue;
    }
    if (content[i] === '}' && content[i + 1] === '}' && content[i + 2] === '}') {
      depth--;
      i += 3;
      if (depth === 0) return i;
      continue;
    }
    i++;
  }
  return null;
}

function skipComment(content: string, start: number): number | null {
  if (content[start] !== '<' || content[start + 1] !== '!' || content[start + 2] !== '-' || content[start + 3] !== '-') {
    return null;
  }
  let i = start + 4;
  while (i < content.length - 2) {
    if (content[i] === '-' && content[i + 1] === '-' && content[i + 2] === '>') {
      return i + 3;
    }
    i++;
  }
  return null;
}

function skipTag(content: string, start: number): number | null {
  if (content[start] !== '<') return null;
  let i = start + 1;
  if (content[i] === '/') return null;

  const nameStart = i;
  while (i < content.length && isTagNameChar(content[i])) i++;
  if (i === nameStart) return null;

  const tagName = content.slice(nameStart, i).toLowerCase();
  if (!SKIP_TAGS.has(tagName)) return null;

  const openEnd = findChar(content, '>', i);
  if (openEnd === null) return null;

  const closeTag = `</${tagName}>`;
  const closeIndex = indexOfIgnoreCase(content, closeTag, openEnd + 1);
  if (closeIndex === -1) return null;

  return closeIndex + closeTag.length;
}

function isTagNameChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
}

function findChar(text: string, target: string, start: number): number | null {
  for (let i = start; i < text.length; i++) {
    if (text[i] === target) return i;
  }
  return null;
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

function extractTagContent(text: string, tag: string): string | null {
  const openTag = `<${tag}`;
  const closeTag = `</${tag}>`;
  const openIndex = indexOfIgnoreCase(text, openTag, 0);
  if (openIndex === -1) return null;

  const startAfterOpen = findChar(text, '>', openIndex + openTag.length);
  if (startAfterOpen === null) return null;

  const closeIndex = indexOfIgnoreCase(text, closeTag, startAfterOpen + 1);
  if (closeIndex === -1) return null;

  const raw = text.slice(startAfterOpen + 1, closeIndex).trim();
  return raw || null;
}

function isIdentStart(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || ch === '_';
}

function isIdentChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return isIdentStart(ch) || (code >= 48 && code <= 57);
}

function skipWhitespace(text: string, start: number): number {
  let i = start;
  while (i < text.length && isWhitespace(text[i])) i++;
  return i;
}

function skipLine(text: string, start: number): number {
  let i = start;
  while (i < text.length && text[i] !== '\n') i++;
  return i;
}

function skipString(text: string, start: number, quote: string): number | null {
  let i = start + 1;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '\\') {
      i += 2;
      continue;
    }
    if (ch === quote) {
      return i + 1;
    }
    i++;
  }
  return null;
}

function findLongBracketEnd(text: string, start: number): number | null {
  if (text[start] !== '[' || text[start + 1] !== '[') return null;
  let i = start + 2;
  while (i < text.length - 1) {
    if (text[i] === ']' && text[i + 1] === ']') {
      return i + 2;
    }
    i++;
  }
  return null;
}

function readStringArgument(text: string, start: number): { value: string; nextIndex: number } | null {
  let i = skipWhitespace(text, start);
  if (text[i] === '(') {
    i = skipWhitespace(text, i + 1);
  }

  const quote = text[i];
  if (quote !== '\'' && quote !== '"') return null;

  let value = '';
  i++;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '\\') {
      const next = text[i + 1];
      if (next) {
        value += next;
        i += 2;
        continue;
      }
      i++;
      continue;
    }
    if (ch === quote) {
      return { value, nextIndex: i + 1 };
    }
    value += ch;
    i++;
  }

  return null;
}
