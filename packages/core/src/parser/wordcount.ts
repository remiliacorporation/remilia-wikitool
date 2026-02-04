/**
 * Word Count Calculation
 *
 * Calculates word count for visible prose only.
 */

function isWordChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (
    (code >= 48 && code <= 57) ||
    (code >= 65 && code <= 90) ||
    (code >= 97 && code <= 122)
  );
}

function countWordsInText(text: string): number {
  let count = 0;
  let inWord = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (isWordChar(ch)) {
      if (!inWord) {
        count++;
        inWord = true;
      }
    } else {
      inWord = false;
    }
  }
  return count;
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

function skipTag(text: string, start: number): { nextIndex: number; tagName: string } | null {
  if (text[start] !== '<') return null;
  let i = start + 1;
  let isClosing = false;
  if (text[i] === '/') {
    isClosing = true;
    i++;
  }
  const nameStart = i;
  while (i < text.length && isTagNameChar(text[i])) i++;
  if (i === nameStart) return null;
  const tagName = text.slice(nameStart, i).toLowerCase();
  while (i < text.length && text[i] !== '>') i++;
  if (i >= text.length) return null;
  return { nextIndex: i + 1, tagName: isClosing ? `/${tagName}` : tagName };
}

function isTagNameChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
}

function skipComment(text: string, start: number): number | null {
  if (!startsWithAt(text, start, '<!--')) return null;
  const end = indexOfSeq(text, '-->', start + 4);
  return end === -1 ? null : end + 3;
}

function readUntil(text: string, start: number, endSeq: string): number | null {
  const end = indexOfSeq(text, endSeq, start);
  return end === -1 ? null : end + endSeq.length;
}

function parseInternalLink(text: string, start: number): { display: string; nextIndex: number } | null {
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
        return { display, nextIndex: i + 2 };
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

function selectLinkDisplay(raw: string): string {
  const pipeIndex = raw.lastIndexOf('|');
  if (pipeIndex !== -1) {
    return raw.slice(pipeIndex + 1);
  }

  const hashIndex = raw.indexOf('#');
  const base = hashIndex !== -1 ? raw.slice(0, hashIndex) : raw;

  const trimmed = base.trim();
  const lower = trimmed.toLowerCase();
  if (startsWithAt(lower, 0, 'category:')) return '';
  if (startsWithAt(lower, 0, 'file:')) return '';
  if (startsWithAt(lower, 0, 'image:')) return '';

  return trimmed;
}

function parseExternalLink(text: string, start: number): { display: string; nextIndex: number } | null {
  let i = start;
  let buffer = '';
  while (i < text.length) {
    if (text[i] === ']') {
      const display = selectExternalLinkDisplay(buffer);
      return { display, nextIndex: i + 1 };
    }
    buffer += text[i];
    i++;
  }
  return null;
}

function selectExternalLinkDisplay(raw: string): string {
  let i = 0;
  while (i < raw.length && raw[i] === ' ') i++;
  const spaceIndex = raw.indexOf(' ', i);
  if (spaceIndex === -1) return '';
  return raw.slice(spaceIndex + 1).trim();
}

function isMagicWord(text: string, start: number): number | null {
  if (text[start] !== '_' || text[start + 1] !== '_') return null;
  let i = start + 2;
  while (i < text.length && text[i] === '_') i++;
  while (i < text.length && isMagicWordChar(text[i])) i++;
  if (text[i] === '_' && text[i + 1] === '_') {
    let j = i + 2;
    while (j < text.length && text[j] === '_') j++;
    return j;
  }
  return null;
}

function isMagicWordChar(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return (code >= 65 && code <= 90) || ch === '_';
}

/**
 * Calculate word count for visible prose
 */
export function calculateWordCount(content: string): number {
  let count = 0;
  let inWord = false;
  let templateDepth = 0;
  let i = 0;

  while (i < content.length) {
    const ch = content[i];

    // Comments
    if (ch === '<') {
      const commentSkip = skipComment(content, i);
      if (commentSkip !== null) {
        i = commentSkip;
        inWord = false;
        continue;
      }
    }

    // Tags with content to skip
    if (ch === '<') {
      const tag = skipTag(content, i);
      if (tag) {
        const tagName = tag.tagName;
        if (tagName === 'nowiki' || tagName === 'ref' || tagName === 'gallery') {
          const end = readUntil(content, tag.nextIndex, `</${tagName}>`);
          if (end !== null) {
            i = end;
            inWord = false;
            continue;
          }
        }
        i = tag.nextIndex;
        continue;
      }
    }

    // Templates (skip fully)
    if (ch === '{' && content[i + 1] === '{') {
      templateDepth++;
      i += 2;
      inWord = false;
      continue;
    }
    if (ch === '}' && content[i + 1] === '}') {
      if (templateDepth > 0) templateDepth--;
      i += 2;
      inWord = false;
      continue;
    }
    if (templateDepth > 0) {
      i++;
      continue;
    }

    // Magic words
    if (ch === '_') {
      const magicEnd = isMagicWord(content, i);
      if (magicEnd !== null) {
        i = magicEnd;
        inWord = false;
        continue;
      }
    }

    // Internal links
    if (ch === '[' && content[i + 1] === '[') {
      const link = parseInternalLink(content, i + 2);
      if (link) {
        count += countWordsInText(link.display);
        i = link.nextIndex;
        inWord = false;
        continue;
      }
    }

    // External links
    if (ch === '[') {
      const ext = parseExternalLink(content, i + 1);
      if (ext) {
        count += countWordsInText(ext.display);
        i = ext.nextIndex;
        inWord = false;
        continue;
      }
    }

    // Visible text
    if (isWordChar(ch)) {
      if (!inWord) {
        count++;
        inWord = true;
      }
    } else {
      inWord = false;
    }

    i++;
  }

  return count;
}