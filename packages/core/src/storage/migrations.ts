/**
 * Enhanced migration runner for wikitool
 *
 * Provides transactional migrations with proper tracking,
 * rollback on failure, and schema validation.
 */

import type { Database as BunDatabase } from 'bun:sqlite';
import { MIGRATIONS } from './schema.js';
import { execSql } from './utils.js';

/** Migration record from schema_migrations table */
export interface MigrationRecord {
  version: string;
  applied_at: string;
}

/** Result of running migrations */
export interface MigrationResult {
  applied: string[];
  skipped: string[];
  failed?: { version: string; error: string };
}

/** Schema validation result */
export interface SchemaValidation {
  valid: boolean;
  currentVersion: string;
  expectedVersion: string;
  missingTables?: string[];
}

/**
 * Get applied migrations from database
 */
export function getAppliedMigrations(db: BunDatabase): Set<string> {
  try {
    const rows = db.prepare(
      'SELECT version FROM schema_migrations WHERE 1=1'
    ).all() as { version: string }[];
    return new Set(rows.map(r => r.version));
  } catch {
    // Table doesn't exist yet - will be created by first migration
    return new Set();
  }
}

/**
 * Get migration history with timestamps
 */
export function getMigrationHistory(db: BunDatabase): MigrationRecord[] {
  try {
    return db.prepare(
      'SELECT version, applied_at FROM schema_migrations ORDER BY version'
    ).all() as MigrationRecord[];
  } catch {
    return [];
  }
}

/**
 * Run pending migrations with proper tracking
 *
 * Each migration runs in its own transaction. On failure, that
 * specific migration is rolled back and the error is reported.
 */
export function runMigrations(db: BunDatabase): MigrationResult {
  const applied: string[] = [];
  const skipped: string[] = [];

  const appliedSet = getAppliedMigrations(db);

  for (const migration of MIGRATIONS) {
    if (appliedSet.has(migration.version)) {
      skipped.push(migration.version);
      continue;
    }

    console.log(`Running migration: ${migration.version}`);

    try {
      // Run migration in transaction
      db.run('BEGIN IMMEDIATE');
      execSql(db, migration.sql);

      // Record success in schema_migrations table (created by v001)
      // Use INSERT OR REPLACE in case it somehow already exists
      db.prepare(
        `INSERT OR REPLACE INTO schema_migrations (version, applied_at)
         VALUES (?, datetime('now'))`
      ).run(migration.version);

      // Update config.schema_version
      db.prepare(
        `UPDATE config SET value = ?, updated_at = datetime('now')
         WHERE key = 'schema_version'`
      ).run(migration.version);

      db.run('COMMIT');
      applied.push(migration.version);

    } catch (error) {
      db.run('ROLLBACK');
      const message = error instanceof Error ? error.message : String(error);
      console.error(`Migration ${migration.version} failed: ${message}`);
      return {
        applied,
        skipped,
        failed: { version: migration.version, error: message }
      };
    }
  }

  return { applied, skipped };
}

/**
 * Get current schema version from config table
 */
export function getSchemaVersion(db: BunDatabase): string {
  try {
    const row = db.prepare(
      'SELECT value FROM config WHERE key = ?'
    ).get('schema_version') as { value: string } | undefined;
    return row?.value ?? '000';
  } catch {
    return '000';
  }
}

/**
 * Get expected (latest) schema version
 */
export function getExpectedVersion(): string {
  return MIGRATIONS[MIGRATIONS.length - 1].version;
}

/**
 * Validate database schema matches expected version
 */
export function validateSchema(db: BunDatabase): SchemaValidation {
  const currentVersion = getSchemaVersion(db);
  const expectedVersion = getExpectedVersion();

  // Check for required tables
  const requiredTables = [
    'pages',
    'categories',
    'page_categories',
    'config',
    'schema_migrations',
    'sync_log',
    'fetch_cache',
    'page_sections',
    'page_sections_fts',
    'template_calls',
    'template_params',
    'infobox_kv',
    'template_metadata',
    'module_deps',
  ];

  const existingTables = new Set(
    (db.prepare(
      "SELECT name FROM sqlite_master WHERE type='table'"
    ).all() as { name: string }[]).map(r => r.name)
  );

  const missingTables = requiredTables.filter(t => !existingTables.has(t));

  return {
    valid: currentVersion === expectedVersion && missingTables.length === 0,
    currentVersion,
    expectedVersion,
    missingTables: missingTables.length > 0 ? missingTables : undefined,
  };
}

/**
 * Check if there are pending migrations
 */
export function hasPendingMigrations(db: BunDatabase): boolean {
  const appliedSet = getAppliedMigrations(db);
  return MIGRATIONS.some(m => !appliedSet.has(m.version));
}

/**
 * Get list of pending migration versions
 */
export function getPendingMigrations(db: BunDatabase): string[] {
  const appliedSet = getAppliedMigrations(db);
  return MIGRATIONS.filter(m => !appliedSet.has(m.version)).map(m => m.version);
}
