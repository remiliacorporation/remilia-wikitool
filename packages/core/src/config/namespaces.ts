/**
 * Namespace Configuration Loader
 *
 * Loads namespace and interwiki configuration from remilia-parser.json at runtime.
 * Falls back to hardcoded defaults if config file is not found.
 */

import { readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// Get __dirname equivalent for ES modules
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

interface ParserConfig {
  namespaces: Record<string, string>; // "0": "", "6": "File"
  nsid: Record<string, number>; // "file": 6, "image": 6
  interwiki: string[]; // ["w", "wikipedia", ...]
}

// Default values (fallback when config not found)
const DEFAULT_NAMESPACES: Record<number, string> = {
  0: '',
  1: 'Talk',
  2: 'User',
  3: 'User talk',
  4: 'Project',
  5: 'Project talk',
  6: 'File',
  7: 'File talk',
  8: 'MediaWiki',
  9: 'MediaWiki talk',
  10: 'Template',
  11: 'Template talk',
  12: 'Help',
  13: 'Help talk',
  14: 'Category',
  15: 'Category talk',
  828: 'Module',
  829: 'Module talk',
  [-2]: 'Media',
  [-1]: 'Special',
};

const DEFAULT_NSID: Record<string, number> = {
  '': 0,
  talk: 1,
  user: 2,
  'user talk': 3,
  project: 4,
  'project talk': 5,
  file: 6,
  image: 6, // alias
  'file talk': 7,
  mediawiki: 8,
  'mediawiki talk': 9,
  template: 10,
  'template talk': 11,
  help: 12,
  'help talk': 13,
  category: 14,
  'category talk': 15,
  module: 828,
  'module talk': 829,
  media: -2,
  special: -1,
};

const DEFAULT_INTERWIKI = new Set([
  'w',
  'wikipedia',
  'wiktionary',
  'wikt',
  'commons',
  'mediawikiwiki',
  'mw',
]);

// Runtime state
let namespaceById: Map<number, string> = new Map();
let namespaceByName: Map<string, number> = new Map();
let interwikiPrefixes: Set<string> = new Set();
let loaded = false;

/**
 * Load namespace configuration from file (call once at startup)
 */
export function loadNamespaceConfig(configPath?: string): void {
  // Try multiple paths
  const paths = configPath
    ? [configPath]
    : [
        join(process.cwd(), 'config/remilia-parser.json'),
        join(process.cwd(), 'wikitool/config/remilia-parser.json'),
        join(process.cwd(), 'custom/wikitool/config/remilia-parser.json'),
        // Also try relative to this file (for when running from different locations)
        join(__dirname, '../../../config/remilia-parser.json'),
      ];

  for (const path of paths) {
    if (existsSync(path)) {
      try {
        const config: ParserConfig = JSON.parse(readFileSync(path, 'utf-8'));

        // Load namespaces: { "0": "", "6": "File" }
        namespaceById.clear();
        for (const [id, name] of Object.entries(config.namespaces)) {
          const nsId = parseInt(id, 10);
          if (!isNaN(nsId)) {
            namespaceById.set(nsId, name);
          }
        }

        // Load nsid: { "file": 6, "image": 6 }
        namespaceByName.clear();
        for (const [name, nsId] of Object.entries(config.nsid)) {
          namespaceByName.set(name.toLowerCase(), nsId);
        }

        // Load interwiki
        interwikiPrefixes = new Set(config.interwiki.map(p => p.toLowerCase()));

        loaded = true;
        return;
      } catch {
        // Failed to load config, continue to next path
      }
    }
  }

  // Fall back to defaults
  initDefaults();
}

function initDefaults(): void {
  namespaceById.clear();
  namespaceByName.clear();

  for (const [id, name] of Object.entries(DEFAULT_NAMESPACES)) {
    const nsId = parseInt(id, 10);
    namespaceById.set(nsId, name);
  }

  for (const [name, nsId] of Object.entries(DEFAULT_NSID)) {
    namespaceByName.set(name.toLowerCase(), nsId);
  }

  interwikiPrefixes = new Set(DEFAULT_INTERWIKI);
  loaded = true;
}

function ensureLoaded(): void {
  if (!loaded) loadNamespaceConfig();
}

// Public API

/**
 * Get namespace ID from name (case-insensitive)
 */
export function getNamespaceId(name: string): number | undefined {
  ensureLoaded();
  return namespaceByName.get(name.toLowerCase());
}

/**
 * Get namespace name from ID
 */
export function getNamespaceName(nsId: number): string | undefined {
  ensureLoaded();
  return namespaceById.get(nsId);
}

/**
 * Check if a prefix is an interwiki prefix
 */
export function isInterwikiPrefix(prefix: string): boolean {
  ensureLoaded();
  return interwikiPrefixes.has(prefix.toLowerCase());
}

/**
 * Check if namespace ID is the File namespace
 */
export function isFileNamespace(nsId: number): boolean {
  return nsId === 6;
}

/**
 * Check if namespace ID is the Category namespace
 */
export function isCategoryNamespace(nsId: number): boolean {
  return nsId === 14;
}

/**
 * Check if namespace ID is a content namespace (non-negative)
 */
export function isContentNamespace(nsId: number): boolean {
  return nsId >= 0;
}

/**
 * Get all loaded interwiki prefixes
 */
export function getInterwikiPrefixes(): string[] {
  ensureLoaded();
  return [...interwikiPrefixes];
}

/**
 * Get all namespace IDs and names
 */
export function getAllNamespaces(): Map<number, string> {
  ensureLoaded();
  return new Map(namespaceById);
}

// Translation subpage detection - only languages with actual translations
// Keep this minimal to avoid hiding legitimate subpages like /archive or /draft
const TRANSLATION_LANGUAGES = new Set(['ja', 'ko', 'es', 'zh']);

/**
 * Check if a title is a translation subpage (e.g., "Article/ja", "Article/zh")
 * Uses allowlist of known language codes to avoid false positives on regular subpages
 */
export function isTranslationSubpage(title: string): boolean {
  const lastSlash = title.lastIndexOf('/');
  if (lastSlash === -1 || lastSlash === title.length - 1) return false;

  const suffix = title.slice(lastSlash + 1).toLowerCase();
  return TRANSLATION_LANGUAGES.has(suffix);
}

/**
 * Get the base title and language code for a translation subpage
 * Returns null if not a translation subpage
 */
export function parseTranslationSubpage(
  title: string
): { baseTitle: string; lang: string } | null {
  if (!isTranslationSubpage(title)) return null;

  const lastSlash = title.lastIndexOf('/');
  return {
    baseTitle: title.slice(0, lastSlash),
    lang: title.slice(lastSlash + 1).toLowerCase(),
  };
}

/**
 * Get all supported translation language codes
 */
export function getTranslationLanguages(): string[] {
  return [...TRANSLATION_LANGUAGES];
}

/**
 * Reset loaded state (for testing)
 */
export function resetNamespaceConfig(): void {
  loaded = false;
  namespaceById.clear();
  namespaceByName.clear();
  interwikiPrefixes.clear();
}
