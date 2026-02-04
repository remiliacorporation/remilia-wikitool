/**
 * Shared SQLite utilities for wikitool
 */

import type { Database as BunDatabase } from 'bun:sqlite';

/**
 * Execute raw SQL on a BunDatabase instance.
 *
 * Handles compatibility between different Bun versions by trying
 * exec() first, then falling back to run().
 */
export function execSql(db: BunDatabase, sql: string): void {
  const anyDb = db as unknown as { exec?: (sql: string) => void; run: (sql: string) => void };
  if (typeof anyDb.exec === 'function') {
    anyDb.exec(sql);
    return;
  }
  anyDb.run(sql);
}
