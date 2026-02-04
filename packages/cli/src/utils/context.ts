/**
 * CLI context utilities
 *
 * Provides shared context (database, filesystem, client) for CLI commands.
 */

import { basename, dirname, resolve } from 'node:path';
import { existsSync, readFileSync } from 'node:fs';
import {
  createDatabase,
  createFilesystem,
  createClientFromEnv,
  createAuthenticatedClient,
  createSyncEngine,
  type Database,
  type Filesystem,
  type MediaWikiClient,
  type SyncEngine,
} from '@wikitool/core';

/** CLI context with all dependencies */
export interface CliContext {
  db: Database;
  fs: Filesystem;
  client: MediaWikiClient;
  engine: SyncEngine;
  rootDir: string;
  dbPath: string;
  projectContext: ProjectContext;
}

export interface ProjectContext {
  /** Whether wikitool is standalone or embedded in a larger repo */
  mode: 'standalone' | 'embedded';
  /** Project root where wiki_content lives (or will live) */
  projectRoot: string;
  /** Directory where wikitool package.json lives */
  wikitoolRoot: string;
  /** Absolute path to wiki_content directory */
  contentDir: string;
  /** Absolute path to templates directory */
  templatesDir: string;
  /** Relative content path (for Filesystem config) */
  contentDirRel: string;
  /** Relative templates path (for Filesystem config) */
  templatesDirRel: string;
  /** Absolute path to SQLite database */
  dbPath: string;
  /** Absolute path to parser config file */
  configPath: string;
}

export function detectProjectContext(startDir: string = process.cwd()): ProjectContext {
  if (process.env.WIKITOOL_PROJECT_ROOT) {
    return buildContextFromEnv(process.env.WIKITOOL_PROJECT_ROOT);
  }

  const wikitoolRoot = findWikitoolRoot(startDir);
  assertWikitoolRoot(wikitoolRoot);

  const isEmbedded = basename(wikitoolRoot) === 'wikitool'
    && basename(dirname(wikitoolRoot)) === 'custom';

  if (isEmbedded) {
    const projectRoot = dirname(dirname(wikitoolRoot));
    return {
      mode: 'embedded',
      projectRoot,
      wikitoolRoot,
      contentDir: resolve(projectRoot, 'wiki_content'),
      templatesDir: resolve(projectRoot, 'custom/templates'),
      contentDirRel: 'wiki_content',
      templatesDirRel: 'custom/templates',
      dbPath: process.env.WIKITOOL_DB || resolve(wikitoolRoot, 'data/wikitool.db'),
      configPath: resolve(wikitoolRoot, 'config/remilia-parser.json'),
    };
  }

  const projectRoot = dirname(wikitoolRoot);
  return {
    mode: 'standalone',
    projectRoot,
    wikitoolRoot,
    contentDir: resolve(projectRoot, 'wiki_content'),
    templatesDir: resolve(projectRoot, 'templates'),
    contentDirRel: 'wiki_content',
    templatesDirRel: 'templates',
    dbPath: process.env.WIKITOOL_DB || resolve(wikitoolRoot, 'data/wikitool.db'),
    configPath: resolve(wikitoolRoot, 'config/remilia-parser.json'),
  };
}

/**
 * Find the project root (directory containing package.json with wikitool workspace)
 * @deprecated Use detectProjectContext().projectRoot
 */
export function findProjectRoot(startDir: string = process.cwd()): string {
  return detectProjectContext(startDir).projectRoot;
}

/**
 * Get the database path
 * @deprecated Use detectProjectContext().dbPath
 */
export function getDbPath(rootDir: string): string {
  return detectProjectContext(rootDir).dbPath;
}

/**
 * Create CLI context (database, filesystem, client, engine)
 *
 * @param requireAuth Whether to require authentication (for write operations)
 */
export async function createContext(options: { requireAuth?: boolean } = {}): Promise<CliContext> {
  const projectContext = detectProjectContext();

  const db = await createDatabase(projectContext.dbPath);
  const fs = createFilesystem(projectContext.projectRoot, {
    contentDir: projectContext.contentDirRel,
    templatesDir: projectContext.templatesDirRel,
  });
  const client = options.requireAuth
    ? await createAuthenticatedClient()
    : createClientFromEnv();
  const engine = createSyncEngine(db, fs, client);

  return {
    db,
    fs,
    client,
    engine,
    rootDir: projectContext.projectRoot,
    dbPath: projectContext.dbPath,
    projectContext,
  };
}

/**
 * Clean up context (close database)
 */
export function closeContext(ctx: CliContext): void {
  ctx.db.close();
}

/**
 * Run a command with context management
 */
export async function withContext<T>(
  fn: (ctx: CliContext) => Promise<T>,
  options: { requireAuth?: boolean } = {}
): Promise<T> {
  const ctx = await createContext(options);
  try {
    return await fn(ctx);
  } finally {
    closeContext(ctx);
  }
}

function findWikitoolRoot(startDir: string): string {
  const embeddedChild = resolve(startDir, 'custom/wikitool/package.json');
  if (existsSync(embeddedChild)) {
    return resolve(startDir, 'custom/wikitool');
  }

  const standaloneChild = resolve(startDir, 'wikitool/package.json');
  if (existsSync(standaloneChild)) {
    return resolve(startDir, 'wikitool');
  }

  let dir = startDir;
  while (dir !== dirname(dir)) {
    const pkgPath = resolve(dir, 'package.json');
    if (existsSync(pkgPath)) {
      try {
        const pkg = JSON.parse(readFileSync(pkgPath, 'utf-8'));
        if (pkg?.name === 'wikitool') {
          return dir;
        }
      } catch {
        // ignore parse errors and keep walking
      }
    }
    dir = dirname(dir);
  }

  return startDir;
}

function assertWikitoolRoot(root: string): void {
  const pkgPath = resolve(root, 'package.json');
  const corePath = resolve(root, 'packages/core/package.json');
  const cliPath = resolve(root, 'packages/cli/package.json');
  if (!existsSync(pkgPath) || !existsSync(corePath) || !existsSync(cliPath)) {
    throw new Error(
      'Could not locate wikitool root. Run from the repo (or its parent) ' +
      'or set WIKITOOL_PROJECT_ROOT / WIKITOOL_ROOT.'
    );
  }
}

function buildContextFromEnv(projectRoot: string): ProjectContext {
  const resolvedProjectRoot = resolve(projectRoot);
  const wikitoolRoot = process.env.WIKITOOL_ROOT
    ? resolve(resolvedProjectRoot, process.env.WIKITOOL_ROOT)
    : resolve(resolvedProjectRoot, 'wikitool');

  assertWikitoolRoot(wikitoolRoot);
  const embedded = basename(wikitoolRoot) === 'wikitool'
    && basename(dirname(wikitoolRoot)) === 'custom';

  return {
    mode: embedded ? 'embedded' : 'standalone',
    projectRoot: resolvedProjectRoot,
    wikitoolRoot,
    contentDir: resolve(resolvedProjectRoot, 'wiki_content'),
    templatesDir: embedded
      ? resolve(resolvedProjectRoot, 'custom/templates')
      : resolve(resolvedProjectRoot, 'templates'),
    contentDirRel: 'wiki_content',
    templatesDirRel: embedded ? 'custom/templates' : 'templates',
    dbPath: process.env.WIKITOOL_DB || resolve(wikitoolRoot, 'data/wikitool.db'),
    configPath: resolve(wikitoolRoot, 'config/remilia-parser.json'),
  };
}
