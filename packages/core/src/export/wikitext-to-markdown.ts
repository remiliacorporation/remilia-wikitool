/**
 * Wikitext to Markdown Converter
 *
 * Converts MediaWiki wikitext to AI-friendly markdown format.
 * Designed for external wiki documentation export (wowdev.wiki, etc.).
 */

export interface ConvertOptions {
  /** Show template params in output (default: true) */
  preserveTemplateParams?: boolean;
  /** Default language for code blocks (e.g., 'c') */
  codeLanguage?: string;
  /** Remove references entirely instead of converting to footnotes (default: false) */
  stripRefs?: boolean;
  /** Include <noinclude> sections (default: false) */
  includeNoinclude?: boolean;
}

/**
 * Convert MediaWiki wikitext to markdown
 */
export function wikitextToMarkdown(wikitext: string, options: ConvertOptions = {}): string {
  const {
    preserveTemplateParams = true,
    codeLanguage = '',
    stripRefs = false,
    includeNoinclude = false,
  } = options;

  let text = wikitext;

  // Remove <noinclude> sections (or include content if option set)
  if (includeNoinclude) {
    text = text.replace(/<\/?noinclude>/gi, '');
  } else {
    text = text.replace(/<noinclude>[\s\S]*?<\/noinclude>/gi, '');
  }

  // Remove <includeonly> tags but keep content
  text = text.replace(/<\/?includeonly>/gi, '');

  // Remove <onlyinclude> tags but keep content
  text = text.replace(/<\/?onlyinclude>/gi, '');

  // Remove magic words
  text = text.replace(/__(?:TOC|NOTOC|FORCETOC|NOEDITSECTION|NOGALLERY|NOTITLECONVERT|NOTC|NOCONTENTCONVERT|NOCC|NEWSECTIONLINK|NONEWSECTIONLINK|HIDDENCAT|STATICREDIRECT|INDEX|NOINDEX|DISAMBIG)__/gi, '');

  // Handle <source> and <syntaxhighlight> code blocks
  text = convertCodeBlocks(text, codeLanguage);

  // Handle <pre> blocks
  text = text.replace(/<pre>([\s\S]*?)<\/pre>/gi, (_, content) => {
    return '\n```\n' + content.trim() + '\n```\n';
  });

  // Handle <code> inline
  text = text.replace(/<code>(.*?)<\/code>/gi, '`$1`');

  // Handle <nowiki> - remove tags but keep content
  text = text.replace(/<nowiki>([\s\S]*?)<\/nowiki>/gi, '$1');

  // Handle references
  if (stripRefs) {
    // Remove references entirely
    text = text.replace(/<ref[^>]*>[\s\S]*?<\/ref>/gi, '');
    text = text.replace(/<ref[^>]*\/>/gi, '');
  } else {
    // Convert to footnote markers (simplified)
    let refNum = 0;
    text = text.replace(/<ref[^>]*>[\s\S]*?<\/ref>/gi, () => `[^${++refNum}]`);
    text = text.replace(/<ref[^>]*\/>/gi, () => `[^${++refNum}]`);
  }

  // Remove <references/> tag
  text = text.replace(/<references\s*\/?>/gi, '');

  // Handle HTML comments
  text = text.replace(/<!--[\s\S]*?-->/g, '');

  // Handle templates before other conversions
  text = convertTemplates(text, preserveTemplateParams);

  // Convert headings (must be done carefully to avoid double-conversion)
  text = convertHeadings(text);

  // Convert bold/italic (order matters: bold-italic first)
  text = text.replace(/'''''([^']+)'''''/g, '***$1***');  // bold-italic
  text = text.replace(/'''([^']+)'''/g, '**$1**');       // bold
  text = text.replace(/''([^']+)''/g, '*$1*');           // italic

  // Convert links
  text = convertLinks(text);

  // Convert lists
  text = convertLists(text);

  // Convert tables
  text = convertTables(text);

  // Convert horizontal rules
  text = text.replace(/^----+$/gm, '---');

  // Strip remaining HTML tags (div, span, etc.)
  text = text.replace(/<\/?(?:div|span|p|br|center|small|big|u|s|strike|tt|blockquote|font)[^>]*>/gi, '');

  // Clean up: remove excess blank lines
  text = text.replace(/\n{4,}/g, '\n\n\n');

  // Trim
  text = text.trim();

  return text;
}

/**
 * Convert <source> and <syntaxhighlight> blocks to fenced code blocks
 */
function convertCodeBlocks(text: string, defaultLang: string): string {
  // Match <source lang="x"> or <syntaxhighlight lang="x">
  const codeBlockRegex = /<(?:source|syntaxhighlight)(?:\s+lang=["']?([^"'\s>]+)["']?)?[^>]*>([\s\S]*?)<\/(?:source|syntaxhighlight)>/gi;

  return text.replace(codeBlockRegex, (_, lang, content) => {
    const language = lang || defaultLang;
    return '\n```' + language + '\n' + content.trim() + '\n```\n';
  });
}

/**
 * Convert MediaWiki headings to markdown
 */
function convertHeadings(text: string): string {
  // Process from deepest (6) to shallowest (2) to avoid double-conversion
  // == Heading == -> ## Heading
  for (let level = 6; level >= 2; level--) {
    const wikiMarker = '='.repeat(level);
    const mdMarker = '#'.repeat(level);
    const regex = new RegExp(`^${wikiMarker}\\s*(.+?)\\s*${wikiMarker}\\s*$`, 'gm');
    text = text.replace(regex, `${mdMarker} $1`);
  }
  return text;
}

/**
 * Convert MediaWiki links to markdown/plain text
 */
function convertLinks(text: string): string {
  // External links with text: [http://example.com Text] -> [Text](http://example.com)
  text = text.replace(/\[(\S+?)\s+([^\]]+)\]/g, '[$2]($1)');

  // External links without text: [http://example.com] -> http://example.com
  text = text.replace(/\[(https?:\/\/[^\s\]]+)\]/g, '$1');

  // Internal links with display text: [[Page|Display]] -> Display
  text = text.replace(/\[\[(?:[^|\]]+)\|([^\]]+)\]\]/g, '$1');

  // Internal links (simple): [[Page]] -> Page
  text = text.replace(/\[\[([^\]]+)\]\]/g, '$1');

  // Category links - remove entirely
  text = text.replace(/\[\[Category:[^\]]+\]\]/gi, '');

  // File/Image links - convert to alt text or remove
  text = text.replace(/\[\[(?:File|Image):([^|\]]+)\|?[^\]]*\]\]/gi, '[Image: $1]');

  return text;
}

/**
 * Convert MediaWiki templates to readable format
 */
function convertTemplates(text: string, preserveParams: boolean): string {
  // Track nested templates by processing from innermost to outermost
  let prevText = '';
  while (prevText !== text) {
    prevText = text;
    // Match innermost templates (no nested {{ }})
    text = text.replace(/\{\{([^{}]+)\}\}/g, (_, content) => {
      return convertSingleTemplate(content.trim(), preserveParams);
    });
  }
  return text;
}

/**
 * Convert a single template (already extracted from {{ }})
 */
function convertSingleTemplate(content: string, preserveParams: boolean): string {
  // Split into name and params
  const parts = content.split('|');
  const templateName = parts[0].trim();
  const params = parts.slice(1);

  // Handle specific template patterns

  // Type templates (common in technical wikis like wowdev)
  // {{Template:Type/M2Array|char}} -> M2Array<char>
  if (templateName.startsWith('Template:Type/') || templateName.startsWith('Type/')) {
    const typeName = templateName.replace(/^(?:Template:)?Type\//, '');
    if (params.length > 0) {
      return `${typeName}<${params.join(', ')}>`;
    }
    return typeName;
  }

  // Version templates
  if (templateName.includes('VersionRange') || templateName.includes('Sandbox/VersionRange')) {
    if (params.length > 0 && preserveParams) {
      return `[Version: ${params.join('-')}]`;
    }
    return '';
  }

  // Section link templates
  if (templateName.toLowerCase() === 'section-link' || templateName.toLowerCase() === 'slink') {
    return params.length > 0 ? params[0] : '';
  }

  // Main/See also templates
  if (templateName.toLowerCase() === 'main' || templateName.toLowerCase() === 'see also') {
    return params.length > 0 ? `*See: ${params.join(', ')}*` : '';
  }

  // Citation templates - keep simplified
  if (templateName.toLowerCase().startsWith('cite')) {
    return '[citation]';
  }

  // Note/Warning/Info boxes
  const noteTemplates = ['note', 'warning', 'info', 'tip', 'caution', 'important'];
  if (noteTemplates.some(n => templateName.toLowerCase() === n)) {
    const noteContent = params.join(' ').trim();
    return `> **${templateName}**: ${noteContent}`;
  }

  // Infobox templates - format as key-value list
  if (templateName.toLowerCase().includes('infobox')) {
    return formatInfobox(templateName, params);
  }

  // Nowrap - just return content
  if (templateName.toLowerCase() === 'nowrap') {
    return params.join('');
  }

  // Unknown templates - show name and params if preserveParams
  if (preserveParams && params.length > 0) {
    // Filter out empty params and named params for cleaner output
    const cleanParams = params
      .filter(p => p.trim() && !p.includes('='))
      .map(p => p.trim());
    if (cleanParams.length > 0) {
      return `{{${templateName}: ${cleanParams.join(', ')}}}`;
    }
  }

  // For templates with no useful params, just show the name or nothing
  if (templateName.startsWith('Template:')) {
    return `{{${templateName.replace('Template:', '')}}}`;
  }

  return `{{${templateName}}}`;
}

/**
 * Format infobox templates as a readable list
 */
function formatInfobox(name: string, params: string[]): string {
  const lines: string[] = [];
  lines.push(`**${name.replace(/^(?:Template:)?/i, '')}**`);

  for (const param of params) {
    const match = param.match(/^\s*([^=]+)\s*=\s*(.+)\s*$/);
    if (match) {
      const [, key, value] = match;
      if (value.trim()) {
        lines.push(`- **${key.trim()}**: ${value.trim()}`);
      }
    }
  }

  return lines.join('\n');
}

/**
 * Convert MediaWiki lists to markdown
 */
function convertLists(text: string): string {
  const lines = text.split('\n');
  const result: string[] = [];

  for (const line of lines) {
    // Unordered lists: * item -> - item
    const unorderedMatch = line.match(/^(\*+)\s*(.*)$/);
    if (unorderedMatch) {
      const [, markers, content] = unorderedMatch;
      const indent = '  '.repeat(markers.length - 1);
      result.push(`${indent}- ${content}`);
      continue;
    }

    // Ordered lists: # item -> 1. item
    const orderedMatch = line.match(/^(#+)\s*(.*)$/);
    if (orderedMatch) {
      const [, markers, content] = orderedMatch;
      const indent = '  '.repeat(markers.length - 1);
      result.push(`${indent}1. ${content}`);
      continue;
    }

    // Definition lists: ; term : definition -> **term**: definition
    const defMatch = line.match(/^;\s*([^:]+)\s*:\s*(.*)$/);
    if (defMatch) {
      const [, term, definition] = defMatch;
      result.push(`**${term.trim()}**: ${definition.trim()}`);
      continue;
    }

    // Definition term only: ; term -> **term**
    const termMatch = line.match(/^;\s*(.+)$/);
    if (termMatch) {
      result.push(`**${termMatch[1].trim()}**`);
      continue;
    }

    // Indented text: : text -> > text (blockquote)
    const indentMatch = line.match(/^(:+)\s*(.*)$/);
    if (indentMatch) {
      const [, colons, content] = indentMatch;
      const prefix = '> '.repeat(colons.length);
      result.push(`${prefix}${content}`);
      continue;
    }

    result.push(line);
  }

  return result.join('\n');
}

/**
 * Convert MediaWiki tables to markdown tables
 */
function convertTables(text: string): string {
  // Find table blocks {| ... |}
  const tableRegex = /\{\|[^}]*\n([\s\S]*?)\n\|\}/g;

  return text.replace(tableRegex, (_, tableContent) => {
    return convertTable(tableContent);
  });
}

/**
 * Convert a single MediaWiki table to markdown
 */
function convertTable(content: string): string {
  const lines = content.split('\n');
  const rows: string[][] = [];
  let currentRow: string[] = [];
  let isHeader = false;
  let hasHeaders = false;

  for (const line of lines) {
    const trimmed = line.trim();

    // Skip empty lines and caption
    if (!trimmed || trimmed.startsWith('|+')) continue;

    // New row
    if (trimmed === '|-') {
      if (currentRow.length > 0) {
        rows.push(currentRow);
        currentRow = [];
      }
      isHeader = false;
      continue;
    }

    // Header cells: ! cell
    if (trimmed.startsWith('!')) {
      isHeader = true;
      hasHeaders = true;
      const cells = trimmed.substring(1).split('!!').map(c => extractCellContent(c));
      currentRow.push(...cells);
      continue;
    }

    // Data cells: | cell
    if (trimmed.startsWith('|')) {
      const cells = trimmed.substring(1).split('||').map(c => extractCellContent(c));
      currentRow.push(...cells);
      continue;
    }
  }

  // Don't forget last row
  if (currentRow.length > 0) {
    rows.push(currentRow);
  }

  if (rows.length === 0) return '';

  // Determine column count
  const colCount = Math.max(...rows.map(r => r.length));

  // Normalize rows to have same number of columns
  const normalizedRows = rows.map(row => {
    while (row.length < colCount) row.push('');
    return row;
  });

  // Build markdown table
  const mdLines: string[] = [];

  // If first row is header
  if (hasHeaders && normalizedRows.length > 0) {
    mdLines.push('| ' + normalizedRows[0].join(' | ') + ' |');
    mdLines.push('| ' + normalizedRows[0].map(() => '---').join(' | ') + ' |');

    for (let i = 1; i < normalizedRows.length; i++) {
      mdLines.push('| ' + normalizedRows[i].join(' | ') + ' |');
    }
  } else {
    // No headers - add empty header row
    mdLines.push('| ' + Array(colCount).fill('').join(' | ') + ' |');
    mdLines.push('| ' + Array(colCount).fill('---').join(' | ') + ' |');

    for (const row of normalizedRows) {
      mdLines.push('| ' + row.join(' | ') + ' |');
    }
  }

  return '\n' + mdLines.join('\n') + '\n';
}

/**
 * Extract cell content, removing style attributes
 */
function extractCellContent(cell: string): string {
  // Remove style/class attributes before |
  const pipeIndex = cell.indexOf('|');
  if (pipeIndex > 0 && !cell.startsWith('[[')) {
    // Check if it looks like an attribute (contains = before pipe)
    const beforePipe = cell.substring(0, pipeIndex);
    if (beforePipe.includes('=')) {
      return cell.substring(pipeIndex + 1).trim();
    }
  }
  return cell.trim();
}
