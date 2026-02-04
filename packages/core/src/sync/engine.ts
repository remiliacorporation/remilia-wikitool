/**
 * Sync engine - orchestrates pull/push operations
 *
 * Coordinates between:
 * - MediaWiki API (wiki)
 * - SQLite database (state tracking)
 * - Filesystem (local files)
 */

import type { Database, PageRecord, SyncStatus } from '../storage/sqlite.js';
import type { Filesystem, FileInfo } from '../storage/filesystem.js';
import type { MediaWikiClient } from '../api/client.js';
import type { PageContent } from '../api/types.js';
import { computeHash } from '../storage/sqlite.js';
import {
  Namespace,
  getNamespaceFromTitle,
  getPageType,
  parseRedirect,
  getTemplateCategory,
  TEMPLATE_NAMESPACES,
} from '../models/namespace.js';
import {
  detectLocalChanges,
  detectConflicts,
  getPagesToRefresh,
  timestampsMatch,
  type Change,
  type ChangeType,
} from './diff.js';
import { updatePageIndex } from '../index/update.js';

/** Pull options */
export interface PullOptions {
  /** Namespaces to pull (default: Main only) */
  namespaces?: number[];
  /** Filter by category */
  category?: string;
  /** Full pull (ignore last_pull timestamp) */
  full?: boolean;
  /** Overwrite locally modified files when pulling */
  overwriteLocal?: boolean;
  /** Progress callback */
  onProgress?: (message: string, current?: number, total?: number) => void;
  /** Include templates */
  includeTemplates?: boolean;
}

/** Push options */
export interface PushOptions {
  /** Edit summary */
  summary: string;
  /** Preview only (don't actually push) */
  dryRun?: boolean;
  /** Force push even if wiki has newer changes */
  force?: boolean;
  /** Delete pages on the wiki when local files are removed */
  delete?: boolean;
  /** Include templates */
  includeTemplates?: boolean;
  /** Filter to specific namespaces */
  namespaces?: number[];
  /** Progress callback */
  onProgress?: (message: string, current?: number, total?: number) => void;
}

/** Pull result */
export interface PullResult {
  success: boolean;
  pulled: number;
  skipped: number;
  errors: string[];
  pages: { title: string; action: 'created' | 'updated' | 'skipped' | 'error'; error?: string }[];
}

/** Push result */
export interface PushResult {
  success: boolean;
  pushed: number;
  unchanged: number;
  skipped: number;
  conflicts: string[];
  errors: string[];
  pages: { title: string; action: 'pushed' | 'created' | 'deleted' | 'unchanged' | 'skipped' | 'conflict' | 'error'; error?: string }[];
}

/** Status result */
export interface StatusResult {
  modified: Change[];
  newLocal: Change[];
  conflicts: Change[];
  deletedLocal: Change[];
  synced: number;
  total: number;
}

/**
 * Sync engine for wiki content
 */
export class SyncEngine {
  private db: Database;
  private fs: Filesystem;
  private client: MediaWikiClient;

  /** Track filepaths written during current pull (case-insensitive, for collision detection) */
  private writtenPaths: Map<string, string> = new Map(); // normalized path -> title
  /** Local filepaths by title for overwrite-local pulls */
  private localFilesByTitle: Map<string, string[]> | null = null;

  constructor(db: Database, fs: Filesystem, client: MediaWikiClient) {
    this.db = db;
    this.fs = fs;
    this.client = client;
  }

  /**
   * Get the config key for last-pull timestamp.
   * Category pulls should not affect global incremental state.
   */
  private getPullConfigKey(namespaces: number[], options: PullOptions): string | null {
    if (options.category) {
      return null;
    }

    const key = [...new Set(namespaces)].sort((a, b) => a - b).join('_');
    return `last_pull_ns_${key}`;
  }

  /**
   * Pull pages from wiki
   */
  async pull(options: PullOptions = {}): Promise<PullResult> {
    // Clear the written paths tracker for this pull session
    this.writtenPaths.clear();
    this.localFilesByTitle = null;

    const result: PullResult = {
      success: true,
      pulled: 0,
      skipped: 0,
      errors: [],
      pages: [],
    };

    const namespaces = options.namespaces || [Namespace.Main];
    const pullConfigKey = this.getPullConfigKey(namespaces, options);
    options.onProgress?.('Fetching page list from wiki...');

    // Get list of pages to pull
    let pagesToPull: string[] = [];

    if (options.category) {
      // Pull specific category
      pagesToPull = await this.client.getCategoryMembers(options.category);
      options.onProgress?.(`Found ${pagesToPull.length} pages in category`);
    } else if (!options.full) {
      // Incremental pull - check what's changed since last pull
      const lastPull = pullConfigKey ? this.db.getConfig(pullConfigKey) : null;

      if (lastPull) {
        options.onProgress?.(`Checking for changes since ${lastPull}...`);
        pagesToPull = await this.client.getRecentChanges(lastPull, namespaces);
        options.onProgress?.(`Found ${pagesToPull.length} changed pages`);
      } else {
        // No previous pull, get all pages
        for (const ns of namespaces) {
          const nsPages = await this.client.getAllPages(ns);
          pagesToPull.push(...nsPages);
        }
      }
    } else {
      // Full pull
      for (const ns of namespaces) {
        options.onProgress?.(`Fetching namespace ${ns}...`);
        const nsPages = await this.client.getAllPages(ns);
        pagesToPull.push(...nsPages);
        options.onProgress?.(`Found ${nsPages.length} pages in namespace ${ns}`);
      }
    }

    if (pagesToPull.length === 0) {
      options.onProgress?.('No pages to pull');
      // Don't update last_article_pull timestamp if no pages were found
      // (This allows subsequent incremental pulls to work correctly)
      return result;
    }

    options.onProgress?.(`Pulling ${pagesToPull.length} pages...`);

    // Ensure folder structure exists
    this.fs.ensureContentFolders();
    if (options.includeTemplates) {
      this.fs.ensureTemplateFolders();
    }

    if (options.overwriteLocal) {
      const allLocalFiles = options.includeTemplates
        ? [...this.fs.scanContentFiles(), ...this.fs.scanTemplateFiles()]
        : this.fs.scanContentFiles();
      const filteredFiles = options.namespaces && options.namespaces.length > 0
        ? allLocalFiles.filter(file => options.namespaces!.includes(file.namespace))
        : allLocalFiles;

      this.localFilesByTitle = new Map();
      for (const file of filteredFiles) {
        const list = this.localFilesByTitle.get(file.title) ?? [];
        list.push(file.filepath);
        this.localFilesByTitle.set(file.title, list);
      }
    }

    // Fetch content in batches
    const contents = await this.client.getPageContents(pagesToPull, {
      batchSize: 50,
      onProgress: (completed, total) => {
        options.onProgress?.(`Fetching content...`, completed, total);
      },
    });

    // Process each page
    for (const [title, page] of contents) {
      try {
        await this.processPagePull(page, result, options);
        result.pulled++;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        result.errors.push(`${title}: ${message}`);
        result.pages.push({ title, action: 'error', error: message });
      }
    }

    // Update last pull timestamp only if we actually pulled pages
    if (result.pulled > 0 && pullConfigKey) {
      const now = new Date().toISOString();
      this.db.setConfig(pullConfigKey, now);

      // Back-compat: maintain aggregate timestamps for UI/status.
      const hasMain = namespaces.includes(Namespace.Main);
      const hasTemplates = namespaces.some(ns =>
        ns === Namespace.Template || ns === Namespace.Module || ns === Namespace.MediaWiki
      );
      if (hasMain) {
        this.db.setConfig('last_article_pull', now);
      }
      if (hasTemplates) {
        this.db.setConfig('last_template_pull', now);
      }
    }

    // Log the operation
    this.db.logSync({
      operation: 'pull',
      page_title: null,
      status: result.errors.length === 0 ? 'success' : 'failed',
      revision_id: null,
      error_message: result.errors.length > 0 ? result.errors.join('; ') : null,
      details: JSON.stringify({
        pulled: result.pulled,
        skipped: result.skipped,
        errors: result.errors.length,
      }),
    });

    result.success = result.errors.length === 0;
    this.localFilesByTitle = null;
    return result;
  }

  /**
   * Process a single page during pull
   */
  private async processPagePull(
    page: PageContent,
    result: PullResult,
    options: PullOptions
  ): Promise<void> {
    const [isRedirect, redirectTarget] = parseRedirect(page.content);
    const namespace = page.namespace as Namespace;
    const pageType = getPageType(namespace, isRedirect);
    const contentHash = computeHash(page.content);

    // Skip template/module redirects entirely - they add clutter without value
    // Editors use canonical template names, so redirects aren't needed
    if (isRedirect && TEMPLATE_NAMESPACES.includes(namespace)) {
      result.skipped++;
      result.pages.push({
        title: page.title,
        action: 'skipped',
        error: 'Template redirect skipped'
      });
      return;
    }

    // Determine filepath
    // Redirects go to _redirects subfolder, avoiding case collisions with canonical pages
    const filepath = this.fs.titleToFilepath(page.title, isRedirect);

    // Skip pages that would collide (case-insensitively) with existing files
    // This handles Windows case-insensitivity and avoids data loss
    const normalizedPath = filepath.toLowerCase().replace(/\\/g, '/');

    // Check against existing DB entries
    const existingByPath = this.db.getPageByFilepath(filepath);
    if (existingByPath && existingByPath.title !== page.title) {
      result.skipped++;
      result.pages.push({
        title: page.title,
        action: 'skipped',
        error: `Case collision with "${existingByPath.title}" - skipped`
      });
      return;
    }

    // Check against pages written in this pull session
    const existingInSession = this.writtenPaths.get(normalizedPath);
    if (existingInSession && existingInSession !== page.title) {
      result.skipped++;
      result.pages.push({
        title: page.title,
        action: 'skipped',
        error: `Case collision with "${existingInSession}" - skipped`
      });
      return;
    }

    // Check if we should skip (unchanged)
    const existing = this.db.getPage(page.title);
    if (existing && existing.content_hash === contentHash) {
      const localPaths = this.localFilesByTitle?.get(page.title) ?? [];
      let localModified = false;
      if (localPaths.length === 0) {
        localModified = !!options.overwriteLocal && !this.fs.fileExists(filepath);
      } else {
        localModified = localPaths.some(path => {
          const localInfo = this.fs.readFile(path);
          if (!localInfo) {
            return !!options.overwriteLocal;
          }
          return !!existing.content_hash && localInfo.contentHash !== existing.content_hash;
        });
      }

      if (!localModified || !options.overwriteLocal) {
        result.skipped++;
        result.pages.push({
          title: page.title,
          action: 'skipped',
          error: localModified ? 'Local changes (use --overwrite-local to replace)' : undefined,
        });
        return;
      }
    }

    // Write file
    const mtime = this.fs.writeFile(filepath, page.content);

    // Track this path for collision detection within this session
    this.writtenPaths.set(normalizedPath, page.title);

    // Clean up any alternate local files for this title
    if (options.overwriteLocal && this.localFilesByTitle) {
      const normalizedTarget = filepath.replace(/\\/g, '/').toLowerCase();
      const localPaths = this.localFilesByTitle.get(page.title) ?? [];
      for (const localPath of localPaths) {
        const normalizedLocal = localPath.replace(/\\/g, '/').toLowerCase();
        if (normalizedLocal !== normalizedTarget) {
          this.fs.deleteFile(localPath);
        }
      }
    }

    // Get template category if applicable
    const templateCategory = TEMPLATE_NAMESPACES.includes(namespace)
      ? getTemplateCategory(page.title)
      : null;

    // Update database
    const pageId = this.db.upsertPage({
      title: page.title,
      namespace,
      page_type: pageType,
      filename: filepath.split('/').pop() || '',
      filepath,
      template_category: templateCategory,
      content: page.content,
      content_hash: contentHash,
      file_mtime: mtime,
      wiki_modified_at: page.timestamp,
      last_synced_at: new Date().toISOString(),
      sync_status: 'synced',
      is_redirect: isRedirect ? 1 : 0,
      redirect_target: redirectTarget,
      content_model: page.contentModel,
      page_id: page.pageId,
      revision_id: page.revisionId,
    });

    // Index for full-text search
    this.db.indexPage('content', page.title, page.content);
    updatePageIndex(this.db, {
      id: pageId,
      title: page.title,
      namespace,
      content: page.content,
    });

    result.pages.push({
      title: page.title,
      action: existing ? 'updated' : 'created',
    });
  }

  /**
   * Get local changes (diff command)
   */
  getChanges(options: { includeTemplates?: boolean; namespaces?: number[] } = {}): Change[] {
    let changes = detectLocalChanges(this.db, this.fs, {
      includeTemplates: options.includeTemplates,
    });

    // Filter by namespace if specified
    if (options.namespaces && options.namespaces.length > 0) {
      changes = changes.filter(c => {
        const ns = getNamespaceFromTitle(c.title);
        return options.namespaces!.includes(ns);
      });
    }

    return changes;
  }

  /**
   * Get sync status
   */
  getStatus(options: { includeTemplates?: boolean } = {}): StatusResult {
    const changes = this.getChanges(options);
    const counts = this.db.getPageCounts();

    return {
      modified: changes.filter(c => c.type === 'modified_local'),
      newLocal: changes.filter(c => c.type === 'new_local'),
      conflicts: changes.filter(c => c.type === 'conflict'),
      deletedLocal: changes.filter(c => c.type === 'deleted_local'),
      synced: counts.synced,
      total: Object.values(counts).reduce((a, b) => a + b, 0),
    };
  }

  /**
   * Push local changes to wiki
   */
  async push(options: PushOptions): Promise<PushResult> {
    const result: PushResult = {
      success: true,
      pushed: 0,
      unchanged: 0,
      skipped: 0,
      conflicts: [],
      errors: [],
      pages: [],
    };

    // Get local changes
    const changes = this.getChanges({
      includeTemplates: options.includeTemplates,
      namespaces: options.namespaces,
    });
    const pushableChanges = changes.filter(c =>
      c.type === 'modified_local' || c.type === 'new_local'
    );
    const deletions = options.delete
      ? changes.filter(c => c.type === 'deleted_local')
      : [];

    const totalChanges = pushableChanges.length + deletions.length;
    if (totalChanges === 0) {
      options.onProgress?.('No changes to push');
      return result;
    }

    options.onProgress?.(`Found ${totalChanges} changes to push`);

    // Check for conflicts unless --force
    if (!options.force) {
      options.onProgress?.('Checking for conflicts...');

      const conflictCandidates = [...pushableChanges, ...deletions];
      const titles = conflictCandidates
        .filter(c => c.type === 'modified_local' || c.type === 'deleted_local')
        .map(c => c.title);

      if (titles.length > 0) {
        const wikiTimestamps = await this.client.getPageTimestamps(titles);
        const withConflicts = detectConflicts(conflictCandidates, wikiTimestamps, this.db);

        const conflicts = withConflicts.filter(c => c.type === 'conflict');
        if (conflicts.length > 0) {
          result.conflicts = conflicts.map(c => c.title);
          result.success = false;

          for (const conflict of conflicts) {
            result.pages.push({ title: conflict.title, action: 'conflict' });
          }

          // Still process non-conflicting changes
          const nonConflicts = withConflicts.filter(c => c.type !== 'conflict');
          for (const change of nonConflicts) {
            if (options.dryRun) {
              result.pages.push({
                title: change.title,
                action: change.type === 'deleted_local'
                  ? 'deleted'
                  : (change.type === 'new_local' ? 'created' : 'pushed'),
              });
              result.skipped++;
            } else {
              if (change.type === 'deleted_local') {
                await this.processPageDelete(change, options.summary, result);
                result.pushed++;
              } else {
                await this.processPagePush(change, options.summary, result);
                result.pushed++;
              }
            }
          }

          return result;
        }
      }
    }

    // Dry run - just report what would be done
    if (options.dryRun) {
      for (const change of pushableChanges) {
        result.pages.push({
          title: change.title,
          action: change.type === 'new_local' ? 'created' : 'pushed',
        });
        result.skipped++;
      }
      for (const change of deletions) {
        result.pages.push({
          title: change.title,
          action: 'deleted',
        });
        result.skipped++;
      }
      return result;
    }

    // Push each change
    const allChanges = [...pushableChanges, ...deletions];
    for (let i = 0; i < allChanges.length; i++) {
      const change = allChanges[i];
      options.onProgress?.(`Pushing...`, i + 1, allChanges.length);

      try {
        if (change.type === 'deleted_local') {
          await this.processPageDelete(change, options.summary, result);
          result.pushed++;
        } else {
          await this.processPagePush(change, options.summary, result);
          // Check the action that was set by processPagePush
          const lastPage = result.pages[result.pages.length - 1];
          if (lastPage.action === 'unchanged') {
            result.unchanged++;
          } else {
            result.pushed++;
          }
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        result.errors.push(`${change.title}: ${message}`);
        result.pages.push({ title: change.title, action: 'error', error: message });
      }
    }

    // Log the operation
    this.db.logSync({
      operation: 'push',
      page_title: null,
      status: result.errors.length === 0 ? 'success' : 'failed',
      revision_id: null,
      error_message: result.errors.length > 0 ? result.errors.join('; ') : null,
      details: JSON.stringify({
        pushed: result.pushed,
        unchanged: result.unchanged,
        skipped: result.skipped,
        conflicts: result.conflicts.length,
        errors: result.errors.length,
      }),
    });

    result.success = result.errors.length === 0 && result.conflicts.length === 0;
    return result;
  }

  /**
   * Process a single page during push
   */
  private async processPagePush(change: Change, summary: string, result: PushResult): Promise<void> {
    // Read current file content
    const fileInfo = this.fs.readFile(change.filepath);
    if (!fileInfo) {
      throw new Error(`File not found: ${change.filepath}`);
    }

    // Get content model for the page
    const dbPage = this.db.getPage(change.title);
    const contentModel = dbPage?.content_model || 'wikitext';

    // Push to wiki
    const editResult = await this.client.editPage(
      change.title,
      fileInfo.content,
      summary,
      { contentModel: contentModel !== 'wikitext' ? contentModel : undefined }
    );

    // Update database
    const [isRedirect, redirectTarget] = parseRedirect(fileInfo.content);
    const namespace = getNamespaceFromTitle(change.title);
    const pageType = getPageType(namespace, isRedirect);

    const pageId = this.db.upsertPage({
      title: change.title,
      namespace,
      page_type: pageType,
      filename: fileInfo.filename,
      filepath: change.filepath,
      content: fileInfo.content,
      content_hash: fileInfo.contentHash,
      file_mtime: fileInfo.mtime,
      wiki_modified_at: editResult.newtimestamp || new Date().toISOString(),
      last_synced_at: new Date().toISOString(),
      sync_status: 'synced',
      is_redirect: isRedirect ? 1 : 0,
      redirect_target: redirectTarget,
      page_id: editResult.pageid,
      revision_id: editResult.newrevid,
    });

    // Update FTS index
    this.db.indexPage('content', change.title, fileInfo.content);
    updatePageIndex(this.db, {
      id: pageId,
      title: change.title,
      namespace,
      content: fileInfo.content,
    });

    // Determine action based on API response
    let action: 'created' | 'pushed' | 'unchanged';
    if (editResult.nochange) {
      action = 'unchanged';
    } else if (change.type === 'new_local') {
      action = 'created';
    } else {
      action = 'pushed';
    }

    result.pages.push({
      title: change.title,
      action,
    });
  }

  /**
   * Process a single page deletion during push
   */
  private async processPageDelete(change: Change, reason: string, result: PushResult): Promise<void> {
    await this.client.deletePage(change.title, reason);
    this.db.deletePage(change.title);

    this.db.logSync({
      operation: 'delete',
      page_title: change.title,
      status: 'success',
      revision_id: null,
      error_message: null,
      details: null,
    });

    result.pages.push({
      title: change.title,
      action: 'deleted',
    });
  }

  /**
   * Initialize the sync state from existing files
   *
   * Scans local files and adds them to the database without fetching from wiki.
   * Useful for setting up initial state when files already exist.
   */
  async initFromFiles(options: { includeTemplates?: boolean } = {}): Promise<{ added: number; errors: string[] }> {
    const result = { added: 0, errors: [] as string[] };

    // Ensure folders exist
    this.fs.ensureContentFolders();
    if (options.includeTemplates) {
      this.fs.ensureTemplateFolders();
    }

    // Scan files
    const files = options.includeTemplates
      ? [...this.fs.scanContentFiles(), ...this.fs.scanTemplateFiles()]
      : this.fs.scanContentFiles();

    for (const file of files) {
      try {
        const namespace = file.namespace;
        const pageType = getPageType(namespace, file.isRedirect);
        const templateCategory = TEMPLATE_NAMESPACES.includes(namespace)
          ? getTemplateCategory(file.title)
          : null;

        const existed = this.db.getPage(file.title) !== null;

        const pageId = this.db.upsertPage({
          title: file.title,
          namespace,
          page_type: pageType,
          filename: file.filename,
          filepath: file.filepath,
          template_category: templateCategory,
          content: file.content,
          content_hash: file.contentHash,
          file_mtime: file.mtime,
          sync_status: 'new', // Mark as new until synced with wiki
          is_redirect: file.isRedirect ? 1 : 0,
          redirect_target: file.redirectTarget,
        });

        // Index for FTS
        this.db.indexPage('content', file.title, file.content);
        updatePageIndex(this.db, {
          id: pageId,
          title: file.title,
          namespace,
          content: file.content,
        });

        if (!existed) {
          result.added++;
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        result.errors.push(`${file.title}: ${message}`);
      }
    }

    // Log initialization
    this.db.logSync({
      operation: 'init',
      page_title: null,
      status: 'success',
      revision_id: null,
      error_message: null,
      details: JSON.stringify({ added: result.added, errors: result.errors.length }),
    });

    return result;
  }
}

/**
 * Create a sync engine with all dependencies
 */
export function createSyncEngine(
  db: Database,
  fs: Filesystem,
  client: MediaWikiClient
): SyncEngine {
  return new SyncEngine(db, fs, client);
}
