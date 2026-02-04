/**
 * Change detection for wiki sync
 *
 * Detects changes between local files, database, and wiki.
 * Uses mtime for fast initial check, hash for verification.
 */

import type { Database, PageRecord, SyncStatus } from '../storage/sqlite.js';
import type { Filesystem, FileInfo } from '../storage/filesystem.js';
import { computeHash } from '../storage/sqlite.js';
import {
  getPageType,
  parseRedirect,
  getTemplateCategory,
  TEMPLATE_NAMESPACES,
} from '../models/namespace.js';

/** Change types */
export type ChangeType =
  | 'new_local'      // File exists locally but not in DB
  | 'new_remote'     // Page exists on wiki but not locally
  | 'modified_local' // Local file changed
  | 'modified_remote'// Wiki page changed
  | 'conflict'       // Both local and wiki changed
  | 'deleted_local'  // File deleted locally
  | 'deleted_remote' // Page deleted on wiki
  | 'synced';        // No changes

/** A detected change */
export interface Change {
  title: string;
  type: ChangeType;
  filepath: string;
  localHash?: string;
  dbHash?: string;
  wikiTimestamp?: string;
  dbTimestamp?: string;
  localMtime?: number;
  dbMtime?: number;
  isRedirect: boolean;
  redirectTarget?: string | null;
}

/** Diff options */
export interface DiffOptions {
  /** Only check specific namespaces */
  namespaces?: number[];
  /** Include templates */
  includeTemplates?: boolean;
  /** Only check files in database (skip filesystem scan) */
  databaseOnly?: boolean;
}

/**
 * Detect changes between local files and database
 *
 * This is a fast local-only check using mtime and hash.
 * It does not contact the wiki.
 */
export function detectLocalChanges(
  db: Database,
  fs: Filesystem,
  options: DiffOptions = {}
): Change[] {
  const changes: Change[] = [];

  // Get all files from filesystem
  const files = options.includeTemplates
    ? [...fs.scanContentFiles(), ...fs.scanTemplateFiles()]
    : fs.scanContentFiles();

  // Filter by namespace if specified
  const filteredFiles = options.namespaces
    ? files.filter(f => options.namespaces!.includes(f.namespace))
    : files;

  // Build a set of files we've seen
  const seenTitles = new Set<string>();

  for (const file of filteredFiles) {
    seenTitles.add(file.title);

    const dbPage = db.getPage(file.title);

    if (!dbPage) {
      // New local file
      changes.push({
        title: file.title,
        type: 'new_local',
        filepath: file.filepath,
        localHash: file.contentHash,
        localMtime: file.mtime,
        isRedirect: file.isRedirect,
        redirectTarget: file.redirectTarget,
      });
      continue;
    }

    // Check for changes using mtime first (fast)
    const mtimeChanged = file.mtime !== dbPage.file_mtime;

    if (mtimeChanged) {
      // Mtime changed, verify with hash
      if (file.contentHash !== dbPage.content_hash) {
        changes.push({
          title: file.title,
          type: 'modified_local',
          filepath: file.filepath,
          localHash: file.contentHash,
          dbHash: dbPage.content_hash ?? undefined,
          dbTimestamp: dbPage.wiki_modified_at ?? undefined,
          localMtime: file.mtime,
          dbMtime: dbPage.file_mtime ?? undefined,
          isRedirect: file.isRedirect,
          redirectTarget: file.redirectTarget,
        });
      }
      // If hash matches but mtime differs, file was touched but content unchanged
      // We'll update mtime in DB during sync
    }
  }

  // Check for deleted files (in DB but not on filesystem)
  // Only check namespaces that were actually scanned
  if (!options.databaseOnly) {
    // Determine which namespaces were scanned based on options
    // Content files: Main (0), Category (14), File (6), User (2), Goldenlight (3000)
    // Template files: Template (10), Module (828), MediaWiki (8)
    const scannedNamespaces = new Set<number>();

    // Content namespaces always scanned
    scannedNamespaces.add(0);  // Main
    scannedNamespaces.add(14); // Category
    scannedNamespaces.add(6);  // File
    scannedNamespaces.add(2);  // User
    scannedNamespaces.add(3000); // Goldenlight

    // Template namespaces only if includeTemplates
    if (options.includeTemplates) {
      scannedNamespaces.add(10);  // Template
      scannedNamespaces.add(828); // Module
      scannedNamespaces.add(8);   // MediaWiki
    }

    const dbPages = db.getPages();

    for (const page of dbPages) {
      // Only check pages in namespaces that were scanned
      if (!scannedNamespaces.has(page.namespace)) {
        continue;
      }

      if (!seenTitles.has(page.title)) {
        // File exists in DB but not on filesystem
        changes.push({
          title: page.title,
          type: 'deleted_local',
          filepath: page.filepath,
          dbHash: page.content_hash ?? undefined,
          dbTimestamp: page.wiki_modified_at ?? undefined,
          dbMtime: page.file_mtime ?? undefined,
          isRedirect: !!page.is_redirect,
          redirectTarget: page.redirect_target,
        });
      }
    }
  }

  return changes;
}

/**
 * Compare local changes against wiki timestamps to detect conflicts
 *
 * @param localChanges Changes detected locally
 * @param wikiTimestamps Map of title -> wiki timestamp from API
 */
export function detectConflicts(
  localChanges: Change[],
  wikiTimestamps: Map<string, { timestamp: string; revisionId: number }>,
  db: Database
): Change[] {
  const result: Change[] = [];

  for (const change of localChanges) {
    const wikiInfo = wikiTimestamps.get(change.title);
    const dbPage = db.getPage(change.title);

    if (change.type === 'modified_local' && wikiInfo && dbPage) {
      // Check if wiki was modified after our last sync
      const dbTimestamp = dbPage.wiki_modified_at;

      if (dbTimestamp && !timestampsMatch(dbTimestamp, wikiInfo.timestamp)) {
        // Wiki was modified - this is a conflict
        result.push({
          ...change,
          type: 'conflict',
          wikiTimestamp: wikiInfo.timestamp,
          dbTimestamp,
        });
        continue;
      }
    }

    if (change.type === 'deleted_local' && wikiInfo && dbPage) {
      const dbTimestamp = dbPage.wiki_modified_at;

      if (dbTimestamp && !timestampsMatch(dbTimestamp, wikiInfo.timestamp)) {
        result.push({
          ...change,
          type: 'conflict',
          wikiTimestamp: wikiInfo.timestamp,
          dbTimestamp,
        });
        continue;
      }
    }

    if (change.type === 'new_local' && wikiInfo) {
      // Page exists on wiki but we're treating it as new locally
      // This shouldn't normally happen after a proper pull
      result.push({
        ...change,
        type: 'conflict',
        wikiTimestamp: wikiInfo.timestamp,
      });
      continue;
    }

    result.push(change);
  }

  return result;
}

/**
 * Detect pages that exist on wiki but not locally (new remote)
 */
export function detectNewRemote(
  wikiTitles: string[],
  db: Database,
  fs: Filesystem
): Change[] {
  const changes: Change[] = [];

  for (const title of wikiTitles) {
    const dbPage = db.getPage(title);

    if (!dbPage) {
      const filepath = fs.titleToFilepath(title, false);

      changes.push({
        title,
        type: 'new_remote',
        filepath,
        isRedirect: false, // Will be determined when content is fetched
      });
    }
  }

  return changes;
}

/**
 * Get pages that need to be pulled (wiki modified or new remote)
 */
export function getPagesToRefresh(
  db: Database,
  wikiTimestamps: Map<string, { timestamp: string; revisionId: number }>
): string[] {
  const toPull: string[] = [];

  for (const [title, wiki] of wikiTimestamps) {
    const dbPage = db.getPage(title);

    if (!dbPage) {
      // New page on wiki
      toPull.push(title);
    } else if (!timestampsMatch(dbPage.wiki_modified_at, wiki.timestamp)) {
      // Wiki was modified after our sync
      toPull.push(title);
    }
  }

  return toPull;
}

/**
 * Summarize changes for display
 */
export function summarizeChanges(changes: Change[]): {
  newLocal: number;
  newRemote: number;
  modifiedLocal: number;
  modifiedRemote: number;
  conflicts: number;
  deletedLocal: number;
  deletedRemote: number;
  synced: number;
  total: number;
} {
  const summary = {
    newLocal: 0,
    newRemote: 0,
    modifiedLocal: 0,
    modifiedRemote: 0,
    conflicts: 0,
    deletedLocal: 0,
    deletedRemote: 0,
    synced: 0,
    total: changes.length,
  };

  for (const change of changes) {
    switch (change.type) {
      case 'new_local': summary.newLocal++; break;
      case 'new_remote': summary.newRemote++; break;
      case 'modified_local': summary.modifiedLocal++; break;
      case 'modified_remote': summary.modifiedRemote++; break;
      case 'conflict': summary.conflicts++; break;
      case 'deleted_local': summary.deletedLocal++; break;
      case 'deleted_remote': summary.deletedRemote++; break;
      case 'synced': summary.synced++; break;
    }
  }

  return summary;
}

/**
 * Filter changes by type
 */
export function filterChanges(changes: Change[], types: ChangeType[]): Change[] {
  return changes.filter(c => types.includes(c.type));
}

/**
 * Parse timestamps and check if they match (within tolerance)
 *
 * Handles legacy timestamp formats from old wiki_sync.py
 */
export function timestampsMatch(
  storedTs: string | null | undefined,
  wikiTs: string | null | undefined,
  toleranceSeconds: number = 30
): boolean {
  if (!storedTs || !wikiTs) return true;

  // If stored timestamp doesn't end with Z, it's from old buggy code
  // that stored local time instead of wiki time. Can't reliably compare.
  if (!storedTs.endsWith('Z')) {
    return true;
  }

  try {
    const storedDate = new Date(storedTs);
    const wikiDate = new Date(wikiTs);
    const diff = Math.abs(wikiDate.getTime() - storedDate.getTime()) / 1000;
    return diff <= toleranceSeconds;
  } catch {
    return true; // Can't compare, assume OK
  }
}
