/**
 * MediaWiki namespace definitions and utilities
 *
 * MediaWiki uses numeric namespace IDs to organize content.
 * This module provides type-safe constants and conversion utilities.
 *
 * NOTE: This module integrates with the config module for runtime namespace
 * lookups. Use getNamespaceIdFromConfig() for dynamic lookups that respect
 * the loaded configuration. The enum and hardcoded values serve as fallbacks.
 */

import {
  getNamespaceId as getNamespaceIdFromConfig,
  getNamespaceName as getNamespaceNameFromConfig,
  isInterwikiPrefix as isInterwikiPrefixFromConfig,
} from '../config/namespaces.js';

/** Standard MediaWiki namespace IDs */
export enum Namespace {
  Main = 0,
  Talk = 1,
  User = 2,
  UserTalk = 3,
  Project = 4,
  ProjectTalk = 5,
  File = 6,
  FileTalk = 7,
  MediaWiki = 8,
  MediaWikiTalk = 9,
  Template = 10,
  TemplateTalk = 11,
  Help = 12,
  HelpTalk = 13,
  Category = 14,
  CategoryTalk = 15,
  Module = 828,
  ModuleTalk = 829,
  // Custom namespace for RemiliaWiki
  Goldenlight = 3000,
  GoldenlightTalk = 3001,
}

/** Namespace prefix strings (empty string for Main namespace) */
export const NAMESPACE_PREFIXES: Record<number, string> = {
  [Namespace.Main]: '',
  [Namespace.Talk]: 'Talk:',
  [Namespace.User]: 'User:',
  [Namespace.UserTalk]: 'User talk:',
  [Namespace.Project]: 'Project:',
  [Namespace.ProjectTalk]: 'Project talk:',
  [Namespace.File]: 'File:',
  [Namespace.FileTalk]: 'File talk:',
  [Namespace.MediaWiki]: 'MediaWiki:',
  [Namespace.MediaWikiTalk]: 'MediaWiki talk:',
  [Namespace.Template]: 'Template:',
  [Namespace.TemplateTalk]: 'Template talk:',
  [Namespace.Help]: 'Help:',
  [Namespace.HelpTalk]: 'Help talk:',
  [Namespace.Category]: 'Category:',
  [Namespace.CategoryTalk]: 'Category talk:',
  [Namespace.Module]: 'Module:',
  [Namespace.ModuleTalk]: 'Module talk:',
  [Namespace.Goldenlight]: 'Goldenlight:',
  [Namespace.GoldenlightTalk]: 'Goldenlight talk:',
};

/** Folder names for wiki_content/ organization */
export const NAMESPACE_FOLDERS: Record<number, string> = {
  [Namespace.Main]: 'Main',
  [Namespace.Category]: 'Category',
  [Namespace.File]: 'File',
  [Namespace.User]: 'User',
  [Namespace.Goldenlight]: 'Goldenlight',
};

/** Namespaces that are stored in custom/templates/ with functional organization */
export const TEMPLATE_NAMESPACES = [
  Namespace.Template,
  Namespace.Module,
  Namespace.MediaWiki,
];

/** Content models for each namespace */
export const CONTENT_MODELS: Record<number, string> = {
  [Namespace.Main]: 'wikitext',
  [Namespace.Template]: 'wikitext',
  [Namespace.Module]: 'Scribunto',
  [Namespace.MediaWiki]: 'wikitext', // Varies: css, javascript for .css/.js files
  [Namespace.Category]: 'wikitext',
  [Namespace.File]: 'wikitext',
  [Namespace.User]: 'wikitext',
};

/** Page types for the database */
export type PageType =
  | 'article'
  | 'template'
  | 'module'
  | 'mediawiki'
  | 'category'
  | 'redirect'
  | 'file';

/** Template category mappings for functional organization in custom/templates/ */
export const TEMPLATE_CATEGORY_MAPPINGS = [
  { prefixes: ['Template:Cite', 'Module:Citation'], category: 'cite', folder: 'cite' },
  { prefixes: ['Template:Ref', 'Template:Efn', 'Module:Reference'], category: 'reference', folder: 'reference' },
  { prefixes: ['Template:Infobox', 'Module:Infobox', 'Module:InfoboxImage'], category: 'infobox', folder: 'infobox' },
  { prefixes: ['Template:About', 'Template:See also', 'Template:Main', 'Template:Further', 'Template:Hatnote', 'Template:Redirect', 'Template:Distinguish', 'Module:Hatnote'], category: 'hatnote', folder: 'hatnote' },
  { prefixes: ['Template:Navbox', 'Template:Navbar', 'Template:Flatlist', 'Template:Hlist', 'Module:Navbox', 'Module:Navbar'], category: 'navbox', folder: 'navbox' },
  { prefixes: ['Template:Blockquote', 'Template:Cquote', 'Template:Quote', 'Template:Poem', 'Template:Verse', 'Module:Quotation'], category: 'quotation', folder: 'quotation' },
  { prefixes: ['Template:Ambox', 'Template:Article quality', 'Template:Stub', 'Template:Update', 'Template:Citation needed', 'Template:Cn', 'Template:Clarify', 'Template:When', 'Template:As of', 'Module:Message'], category: 'message', folder: 'message' },
  { prefixes: ['Template:Sidebar', 'Template:Portal', 'Template:Remilia events', 'Module:Sidebar'], category: 'sidebar', folder: 'sidebar' },
  { prefixes: ['Template:Repost', 'Template:Mirror', 'Template:Goldenlight repost', 'Module:Repost'], category: 'repost', folder: 'repost' },
  { prefixes: ['Template:Etherscan', 'Template:Explorer', 'Template:OpenSea', 'Module:Blockchain'], category: 'blockchain', folder: 'blockchain' },
  { prefixes: ['Template:Birth date', 'Template:Start date', 'Template:End date', 'Module:Age'], category: 'date', folder: 'date' },
  { prefixes: ['Template:Remilia navigation'], category: 'navigation', folder: 'navigation' },
  { prefixes: ['Template:Translation', 'Module:Translation'], category: 'translations', folder: 'translations' },
  { prefixes: ['MediaWiki:'], category: 'mediawiki', folder: 'mediawiki' },
];

/**
 * Get the template category for a wiki title
 */
export function getTemplateCategory(title: string): string {
  for (const mapping of TEMPLATE_CATEGORY_MAPPINGS) {
    if (mapping.prefixes.some(p => title.startsWith(p))) {
      return mapping.category;
    }
  }
  return 'misc';
}

/**
 * Extract namespace from a wiki title
 */
export function getNamespaceFromTitle(title: string): Namespace {
  for (const [ns, prefix] of Object.entries(NAMESPACE_PREFIXES)) {
    if (prefix && title.startsWith(prefix)) {
      return parseInt(ns) as Namespace;
    }
  }
  return Namespace.Main;
}

/**
 * Get the page name without namespace prefix
 */
export function getTitleWithoutNamespace(title: string): string {
  const ns = getNamespaceFromTitle(title);
  const prefix = NAMESPACE_PREFIXES[ns];
  return prefix ? title.slice(prefix.length) : title;
}

/**
 * Convert a title to a safe filename (replace spaces with underscores, handle special chars)
 */
export function titleToFilename(title: string): string {
  // Remove namespace prefix
  const name = getTitleWithoutNamespace(title);
  // Replace spaces with underscores, handle / and :
  return name
    .replace(/ /g, '_')
    .replace(/\//g, '___')
    .replace(/:/g, '--');
}

/**
 * Convert a filename back to wiki title (without namespace prefix)
 */
export function filenameToTitle(filename: string): string {
  // Remove extension
  const name = filename.replace(/\.(wiki|lua|css|js|wikitext)$/, '');
  // Reverse the encoding
  return name
    .replace(/___/g, '/')
    .replace(/--/g, ':')
    .replace(/_/g, ' ');
}

/**
 * Determine the file extension for a page
 */
export function getFileExtension(namespace: Namespace, title: string): string {
  if (namespace === Namespace.Module) {
    // Check if it's a styles subpage
    if (title.endsWith('/styles.css')) {
      return '.css';
    }
    return '.lua';
  }

  if (namespace === Namespace.MediaWiki) {
    if (title.endsWith('.css')) return '.css';
    if (title.endsWith('.js')) return '.js';
    return '.wiki';
  }

  return '.wiki';
}

/**
 * Convert wiki title to filepath
 *
 * @param title Full wiki title (e.g., "Template:Infobox person")
 * @param isRedirect Whether this page is a redirect
 * @param baseDir Base directory for wiki content
 * @param templatesDir Base directory for templates
 */
export function titleToFilepath(
  title: string,
  isRedirect: boolean,
  baseDir: string = 'wiki_content',
  templatesDir: string = 'custom/templates'
): string {
  const namespace = getNamespaceFromTitle(title);

  // Redirects go to _redirects subfolder within their namespace folder
  // This avoids case collisions with canonical pages on Windows
  if (isRedirect) {
    const redirectFilename = titleToFilename(title);

    // For template namespaces, use the templates directory structure
    if (TEMPLATE_NAMESPACES.includes(namespace)) {
      const category = getTemplateCategory(title);
      return `${templatesDir}/${category}/_redirects/${redirectFilename}.wiki`;
    }

    // For content namespaces, use _redirects subfolder
    const folder = NAMESPACE_FOLDERS[namespace] || 'Main';
    return `${baseDir}/${folder}/_redirects/${redirectFilename}.wiki`;
  }

  // Templates/Modules/MediaWiki use functional organization
  if (TEMPLATE_NAMESPACES.includes(namespace)) {
    const category = getTemplateCategory(title);
    const ext = getFileExtension(namespace, title);

    // Build filename with namespace prefix preserved
    let filename: string;
    if (namespace === Namespace.Module) {
      const name = getTitleWithoutNamespace(title);
      if (name.endsWith('/styles.css')) {
        // Module:Foo/styles.css -> Module_Foo_styles.css
        const moduleName = name.slice(0, -11); // Remove /styles.css
        filename = `Module_${moduleName.replace(/ /g, '_')}_styles`;
      } else {
        filename = `Module_${name.replace(/ /g, '_')}`;
      }
    } else if (namespace === Namespace.Template) {
      const name = getTitleWithoutNamespace(title);
      filename = `Template_${name.replace(/ /g, '_')}`;
    } else if (namespace === Namespace.MediaWiki) {
      const name = getTitleWithoutNamespace(title);
      // MediaWiki pages keep their full name (already includes extension for .css/.js)
      // Don't add extension if filename already has .css or .js
      if (name.endsWith('.css') || name.endsWith('.js')) {
        return `${templatesDir}/${category}/${name}`;
      }
      filename = name;
    } else {
      filename = titleToFilename(title);
    }

    return `${templatesDir}/${category}/${filename}${ext}`;
  }

  // Regular content pages use namespace folders
  const folder = NAMESPACE_FOLDERS[namespace] || 'Main';
  const filename = titleToFilename(title);
  const ext = getFileExtension(namespace, title);

  return `${baseDir}/${folder}/${filename}${ext}`;
}

/**
 * Determine page type from namespace and content
 */
export function getPageType(namespace: Namespace, isRedirect: boolean): PageType {
  if (isRedirect) return 'redirect';

  switch (namespace) {
    case Namespace.Template:
      return 'template';
    case Namespace.Module:
      return 'module';
    case Namespace.MediaWiki:
      return 'mediawiki';
    case Namespace.Category:
      return 'category';
    case Namespace.File:
      return 'file';
    default:
      return 'article';
  }
}

/**
 * Check if content is a redirect
 * Returns [isRedirect, target]
 */
export function parseRedirect(content: string): [boolean, string | null] {
  if (!content) return [false, null];

  const trimmed = content.trim();
  if (trimmed.toUpperCase().startsWith('#REDIRECT')) {
    const match = trimmed.match(/\[\[([^\]]+)\]\]/);
    if (match) {
      return [true, match[1]];
    }
  }

  return [false, null];
}

// Re-export config-based functions for convenience
// These use the dynamically loaded configuration from remilia-parser.json
export {
  getNamespaceIdFromConfig,
  getNamespaceNameFromConfig,
  isInterwikiPrefixFromConfig,
};

/**
 * Get namespace ID from name using config (with fallback to hardcoded values)
 * Prefer this over direct NAMESPACE_PREFIXES lookups for runtime flexibility
 */
export function getNamespaceIdByName(name: string): number | undefined {
  // Try config first
  const fromConfig = getNamespaceIdFromConfig(name);
  if (fromConfig !== undefined) {
    return fromConfig;
  }

  // Fall back to hardcoded values
  const lowerName = name.toLowerCase();
  for (const [ns, prefix] of Object.entries(NAMESPACE_PREFIXES)) {
    const prefixName = prefix.replace(/:$/, '').toLowerCase();
    if (prefixName === lowerName) {
      return parseInt(ns);
    }
  }

  return undefined;
}

/**
 * Check if a prefix is an interwiki prefix (uses config)
 */
export function isInterwiki(prefix: string): boolean {
  return isInterwikiPrefixFromConfig(prefix);
}
