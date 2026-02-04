/**
 * Filesystem operations for wiki content files
 *
 * Handles reading/writing .wiki files and tracking modification times.
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync, unlinkSync } from 'node:fs';
import { dirname, join, relative, basename, extname } from 'node:path';
import {
  Namespace,
  NAMESPACE_FOLDERS,
  getNamespaceFromTitle,
  titleToFilepath,
  filenameToTitle,
  parseRedirect,
} from '../models/namespace.js';
import { computeHash } from './sqlite.js';

/** File metadata */
export interface FileInfo {
  filepath: string;
  filename: string;
  content: string;
  contentHash: string;
  mtime: number; // Unix timestamp in milliseconds
  title: string;
  namespace: Namespace;
  isRedirect: boolean;
  redirectTarget: string | null;
}

/** Filesystem configuration */
export interface FilesystemConfig {
  /** Base directory for wiki content (default: wiki_content) */
  contentDir: string;
  /** Base directory for templates (embedded: custom/templates, standalone: templates) */
  templatesDir: string;
  /** Project root directory */
  rootDir: string;
}

/**
 * Filesystem manager for wiki content
 */
export class Filesystem {
  private config: FilesystemConfig;

  constructor(config: FilesystemConfig) {
    this.config = config;
  }

  /**
   * Get absolute path from relative path
   */
  private absPath(relativePath: string): string {
    return join(this.config.rootDir, relativePath);
  }

  /**
   * Get relative path from absolute path
   */
  private relPath(absolutePath: string): string {
    return relative(this.config.rootDir, absolutePath);
  }

  /**
   * Ensure a directory exists
   */
  ensureDir(dirPath: string): void {
    const absDir = this.absPath(dirPath);
    if (!existsSync(absDir)) {
      mkdirSync(absDir, { recursive: true });
    }
  }

  /**
   * Read a file and return its info
   */
  readFile(filepath: string): FileInfo | null {
    const absPath = this.absPath(filepath);

    if (!existsSync(absPath)) {
      return null;
    }

    const stat = statSync(absPath);
    const content = readFileSync(absPath, 'utf-8');
    const [isRedirect, redirectTarget] = parseRedirect(content);

    // Determine title from filepath
    const title = this.filepathToTitle(filepath);
    const namespace = getNamespaceFromTitle(title);

    return {
      filepath,
      filename: basename(filepath),
      content,
      contentHash: computeHash(content),
      mtime: stat.mtimeMs,
      title,
      namespace,
      isRedirect,
      redirectTarget,
    };
  }

  /**
   * Write content to a file
   */
  writeFile(filepath: string, content: string): number {
    const absPath = this.absPath(filepath);
    const dir = dirname(absPath);

    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
    }

    writeFileSync(absPath, content, 'utf-8');

    // Return the new mtime
    return statSync(absPath).mtimeMs;
  }

  /**
   * Delete a file
   */
  deleteFile(filepath: string): boolean {
    const absPath = this.absPath(filepath);

    if (!existsSync(absPath)) {
      return false;
    }

    unlinkSync(absPath);
    return true;
  }

  /**
   * Check if file exists
   */
  fileExists(filepath: string): boolean {
    return existsSync(this.absPath(filepath));
  }

  /**
   * Get file mtime
   */
  getFileMtime(filepath: string): number | null {
    const absPath = this.absPath(filepath);

    if (!existsSync(absPath)) {
      return null;
    }

    return statSync(absPath).mtimeMs;
  }

  /**
   * Convert filepath to wiki title
   */
  filepathToTitle(filepath: string): string {
    // Normalize path separators for cross-platform compatibility
    const normalizedPath = filepath.replace(/\\/g, '/');

    const filename = basename(filepath);
    const ext = extname(filename);
    const nameWithoutExt = filename.slice(0, -ext.length);

    const templatesDir = this.config.templatesDir.replace(/\\/g, '/');
    if (normalizedPath.startsWith(`${templatesDir}/`)) {
      const decodeSegment = (value: string): string => value
        .replace(/___/g, '/')
        .replace(/--/g, ':')
        .replace(/_/g, ' ');

      const stripBaseExtension = (value: string): string =>
        value.replace(/\.(wiki|wikitext|lua|css|js)$/i, '');
      const stripSubpageExtension = (value: string): string =>
        value.replace(/\.(wiki|wikitext|lua)$/i, '');

      const relativePath = normalizedPath.slice(templatesDir.length + 1);
      const rawSegments = relativePath.split('/').filter(Boolean);
      const segments = rawSegments.filter(seg => seg !== '_redirects');
      const category = segments[0];
      const rest = segments.slice(1);

      // MediaWiki namespace files (in mediawiki/ folder)
      if (category === 'mediawiki' || normalizedPath.includes('/mediawiki/')) {
        if (rest.length === 0) {
          if (ext === '.css' || ext === '.js') {
            return `MediaWiki:${filename}`;
          }
          return `MediaWiki:${nameWithoutExt}`;
        }

        const subpages = rest.map((seg, idx) => {
          const value = idx === rest.length - 1 ? stripSubpageExtension(seg) : seg;
          return decodeSegment(value);
        });
        return `MediaWiki:${subpages.join('/')}`;
      }

      const baseIndex = rest.findIndex(seg =>
        seg.startsWith('Template_') || seg.startsWith('Module_')
      );

      if (baseIndex !== -1) {
        const baseSegment = rest[baseIndex];
        const baseExt = extname(baseSegment);
        const baseClean = stripBaseExtension(baseSegment);
        const isModule = baseClean.startsWith('Module_');
        const isTemplate = baseClean.startsWith('Template_');

        if (isModule || isTemplate) {
          const prefixLen = isModule ? 7 : 9;
          let baseNameRaw = baseClean.slice(prefixLen);
          let subpageSegments = rest.slice(baseIndex + 1);

          if (isModule && subpageSegments.length === 0 && baseNameRaw.endsWith('_styles') && baseExt === '.css') {
            baseNameRaw = baseNameRaw.slice(0, -7);
            subpageSegments = ['styles.css'];
          }

          const baseTitle = decodeSegment(baseNameRaw);
          if (subpageSegments.length === 0) {
            return `${isModule ? 'Module' : 'Template'}:${baseTitle}`;
          }

          const subpages = subpageSegments.map((seg, idx) => {
            const value = idx === subpageSegments.length - 1 ? stripSubpageExtension(seg) : seg;
            return decodeSegment(value);
          });

          return `${isModule ? 'Module' : 'Template'}:${baseTitle}/${subpages.join('/')}`;
        }
      }

      if (nameWithoutExt.startsWith('Module_')) {
        // Module_Foo.lua -> Module:Foo
        // Module_Foo_styles.css -> Module:Foo/styles.css
        let moduleName = nameWithoutExt.slice(7); // Remove "Module_"
        if (moduleName.endsWith('_styles') && ext === '.css') {
          moduleName = moduleName.slice(0, -7); // Remove "_styles"
          return `Module:${moduleName.replace(/_/g, ' ')}/styles.css`;
        }
        return `Module:${moduleName.replace(/_/g, ' ')}`;
      }

      if (nameWithoutExt.startsWith('Template_')) {
        // Template_Foo_bar.wiki -> Template:Foo bar
        const templateName = nameWithoutExt.slice(9); // Remove "Template_"
        return `Template:${templateName.replace(/_/g, ' ')}`;
      }
    }

    // Check namespace folders in wiki_content
    for (const [ns, folder] of Object.entries(NAMESPACE_FOLDERS)) {
      if (normalizedPath.includes(`/${folder}/`)) {
        const namespace = parseInt(ns) as Namespace;

        if (namespace === Namespace.Main) {
          return filenameToTitle(nameWithoutExt);
        }

        const prefix = this.getNamespacePrefix(namespace);
        return `${prefix}${filenameToTitle(nameWithoutExt)}`;
      }
    }

    // Redirect folder
    if (normalizedPath.includes('/Redirect/')) {
      return filenameToTitle(nameWithoutExt);
    }

    // Default: treat as main namespace
    return filenameToTitle(nameWithoutExt);
  }

  /**
   * Get namespace prefix
   */
  private getNamespacePrefix(namespace: Namespace): string {
    switch (namespace) {
      case Namespace.Category: return 'Category:';
      case Namespace.File: return 'File:';
      case Namespace.User: return 'User:';
      case Namespace.Template: return 'Template:';
      case Namespace.Module: return 'Module:';
      case Namespace.MediaWiki: return 'MediaWiki:';
      case Namespace.Goldenlight: return 'Goldenlight:';
      default: return '';
    }
  }

  /**
   * Convert wiki title to filepath
   */
  titleToFilepath(title: string, isRedirect: boolean = false): string {
    return titleToFilepath(
      title,
      isRedirect,
      this.config.contentDir,
      this.config.templatesDir
    );
  }

  /**
   * Scan all wiki content files
   *
   * Scans namespace folders (Main/, Category/, etc.) and their _redirects subfolders
   */
  scanContentFiles(): FileInfo[] {
    const files: FileInfo[] = [];

    const contentDir = this.absPath(this.config.contentDir);
    if (!existsSync(contentDir)) {
      return files;
    }

    // Scan namespace folders and their _redirects subfolders
    for (const folder of Object.values(NAMESPACE_FOLDERS)) {
      const folderPath = join(contentDir, folder);
      if (existsSync(folderPath)) {
        // Scan main content files
        files.push(...this.scanDirectory(folderPath, '.wiki'));

        // Scan _redirects subfolder
        const redirectsPath = join(folderPath, '_redirects');
        if (existsSync(redirectsPath)) {
          files.push(...this.scanDirectory(redirectsPath, '.wiki'));
        }
      }
    }

    // Legacy: Also scan old Redirect/ folder for migration compatibility
    const legacyRedirectPath = join(contentDir, 'Redirect');
    if (existsSync(legacyRedirectPath)) {
      files.push(...this.scanDirectory(legacyRedirectPath, '.wiki'));
    }

    return files;
  }

  /**
   * Scan all template files
   */
  scanTemplateFiles(): FileInfo[] {
    const files: FileInfo[] = [];
    const templatesDir = this.absPath(this.config.templatesDir);

    if (!existsSync(templatesDir)) {
      return files;
    }

    // Scan category subdirectories
    const entries = readdirSync(templatesDir, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;

      // Skip legacy redirects folder
      if (entry.name === 'redirects') continue;

      const categoryPath = join(templatesDir, entry.name);

      // Scan for .wiki, .wikitext, .lua, .css, .js files (recursive)
      const categoryFiles = this.scanDirectoryRecursive(categoryPath, ['.wiki', '.wikitext', '.lua', '.css', '.js']);

      for (const file of categoryFiles) {
        const normalizedPath = file.filepath.replace(/\\/g, '/');
        const segments = normalizedPath.split('/');
        const hasTemplateSegment = segments.some(segment => segment.startsWith('Template_'));
        const hasModuleSegment = segments.some(segment => segment.startsWith('Module_'));

        const isSyncable = (
          hasTemplateSegment ||
          hasModuleSegment ||
          normalizedPath.includes('/mediawiki/') ||
          normalizedPath.includes('/_redirects/')
        );

        if (isSyncable) {
          files.push(file);
        }
      }
    }

    return files;
  }

  /**
   * Scan a directory for files with given extensions
   */
  private scanDirectory(dirPath: string, extensions: string | string[]): FileInfo[] {
    const files: FileInfo[] = [];
    const exts = Array.isArray(extensions) ? extensions : [extensions];

    if (!existsSync(dirPath)) {
      return files;
    }

    const entries = readdirSync(dirPath, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.isDirectory()) continue;

      const ext = extname(entry.name);
      if (!exts.includes(ext)) continue;

      const filepath = this.relPath(join(dirPath, entry.name));
      const info = this.readFile(filepath);
      if (info) {
        files.push(info);
      }
    }

    return files;
  }

  /**
   * Scan a directory recursively for files with given extensions
   */
  private scanDirectoryRecursive(dirPath: string, extensions: string | string[]): FileInfo[] {
    const files: FileInfo[] = [];
    const exts = Array.isArray(extensions) ? extensions : [extensions];

    if (!existsSync(dirPath)) {
      return files;
    }

    const entries = readdirSync(dirPath, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = join(dirPath, entry.name);
      if (entry.isDirectory()) {
        files.push(...this.scanDirectoryRecursive(fullPath, exts));
        continue;
      }

      const ext = extname(entry.name);
      if (!exts.includes(ext)) continue;

      const filepath = this.relPath(fullPath);
      const info = this.readFile(filepath);
      if (info) {
        files.push(info);
      }
    }

    return files;
  }

  /**
   * Ensure all namespace folders exist
   */
  ensureContentFolders(): void {
    this.ensureDir(this.config.contentDir);

    for (const folder of Object.values(NAMESPACE_FOLDERS)) {
      this.ensureDir(join(this.config.contentDir, folder));
      // Create _redirects subfolder for each namespace
      this.ensureDir(join(this.config.contentDir, folder, '_redirects'));
    }

    // Legacy: Keep Redirect folder for migration compatibility
    this.ensureDir(join(this.config.contentDir, 'Redirect'));
  }

  /**
   * Ensure template category folders exist
   */
  ensureTemplateFolders(): void {
    this.ensureDir(this.config.templatesDir);

    // Common template categories
    const categories = [
      'cite', 'infobox', 'navbox', 'hatnote', 'quotation',
      'message', 'sidebar', 'repost', 'blockchain', 'date',
      'list', 'reference', 'navigation', 'translations', 'misc', 'mediawiki'
    ];

    for (const category of categories) {
      this.ensureDir(join(this.config.templatesDir, category));
      // Create _redirects subfolder for each category
      this.ensureDir(join(this.config.templatesDir, category, '_redirects'));
    }

    // Legacy: Keep redirects folder for migration compatibility
    this.ensureDir(join(this.config.templatesDir, 'redirects'));
  }
}

/**
 * Create filesystem instance
 *
 * @param rootDir Project root directory
 * @param config Optional path overrides. If not provided, uses embedded-mode defaults.
 */
export function createFilesystem(
  rootDir: string,
  config?: Partial<FilesystemConfig>
): Filesystem {
  return new Filesystem({
    contentDir: config?.contentDir ?? 'wiki_content',
    templatesDir: config?.templatesDir ?? 'custom/templates',
    rootDir,
  });
}
