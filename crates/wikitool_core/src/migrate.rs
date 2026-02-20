use std::fs;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::runtime::ResolvedPaths;

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("migrations/v001_baseline.sql"),
    },
    Migration {
        version: 2,
        name: "indexes",
        sql: include_str!("migrations/v002_indexes.sql"),
    },
    Migration {
        version: 3,
        name: "fts5",
        sql: include_str!("migrations/v003_fts5.sql"),
    },
    Migration {
        version: 4,
        name: "template_categories",
        sql: include_str!("migrations/v004_template_categories.sql"),
    },
];

/// Report returned after running migrations.
#[derive(Debug, Clone)]
pub struct MigrateReport {
    pub applied: Vec<AppliedMigration>,
    pub current_version: u32,
}

#[derive(Debug, Clone)]
pub struct AppliedMigration {
    pub version: u32,
    pub name: String,
}

/// Run all pending migrations against the database at `paths.db_path`.
/// Creates the database and parent directories if they do not exist.
pub fn run_migrations(paths: &ResolvedPaths) -> Result<MigrateReport> {
    ensure_db_parent(paths)?;
    let connection = open_connection(&paths.db_path)?;
    ensure_schema_migrations_table(&connection)?;

    let current = current_version(&connection)?;
    let mut applied = Vec::new();

    for migration in MIGRATIONS {
        if migration.version <= current {
            continue;
        }
        apply_migration(&connection, migration)
            .with_context(|| format!("failed to apply migration v{:03}_{}", migration.version, migration.name))?;
        applied.push(AppliedMigration {
            version: migration.version,
            name: migration.name.to_string(),
        });
    }

    let final_version = current_version(&connection)?;
    Ok(MigrateReport {
        applied,
        current_version: final_version,
    })
}

/// Returns the number of migrations that have not yet been applied.
pub fn pending_migration_count(paths: &ResolvedPaths) -> Result<usize> {
    if !paths.db_path.exists() {
        return Ok(MIGRATIONS.len());
    }
    let connection = open_connection(&paths.db_path)?;
    ensure_schema_migrations_table(&connection)?;
    let current = current_version(&connection)?;
    Ok(MIGRATIONS
        .iter()
        .filter(|m| m.version > current)
        .count())
}

/// Returns the highest applied migration version, or 0 if none applied.
pub fn current_version(connection: &Connection) -> Result<u32> {
    let version: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .context("failed to read current migration version")?;
    u32::try_from(version).context("migration version does not fit into u32")
}

fn ensure_schema_migrations_table(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at_unix INTEGER NOT NULL
            );",
        )
        .context("failed to create schema_migrations table")
}

fn apply_migration(connection: &Connection, migration: &Migration) -> Result<()> {
    connection
        .execute_batch("SAVEPOINT migration_apply")
        .context("failed to create savepoint")?;

    let result = (|| -> Result<()> {
        connection
            .execute_batch(migration.sql)
            .with_context(|| format!("SQL execution failed for v{:03}", migration.version))?;

        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock error")?
            .as_secs();

        connection
            .execute(
                "INSERT INTO schema_migrations (version, name, applied_at_unix) VALUES (?1, ?2, ?3)",
                params![
                    i64::from(migration.version),
                    migration.name,
                    i64::try_from(now_unix).context("timestamp does not fit into i64")?,
                ],
            )
            .context("failed to record migration")?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            connection
                .execute_batch("RELEASE SAVEPOINT migration_apply")
                .context("failed to release savepoint")?;
            Ok(())
        }
        Err(err) => {
            let _ = connection.execute_batch("ROLLBACK TO SAVEPOINT migration_apply");
            let _ = connection.execute_batch("RELEASE SAVEPOINT migration_apply");
            Err(err)
        }
    }
}

fn open_connection(db_path: &std::path::Path) -> Result<Connection> {
    let connection =
        Connection::open(db_path).with_context(|| format!("failed to open {}", db_path.display()))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable foreign_keys pragma")?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .context("failed to enable WAL journal mode")?;
    Ok(connection)
}

fn ensure_db_parent(paths: &ResolvedPaths) -> Result<()> {
    let parent = paths
        .db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("db path has no parent: {}", paths.db_path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create database parent directory {}",
            parent.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn test_paths() -> (tempfile::TempDir, ResolvedPaths) {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
        fs::create_dir_all(project_root.join(".wikitool/data")).expect("create data dir");
        let paths = ResolvedPaths {
            db_path: project_root.join(".wikitool/data/wikitool.db"),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool/data"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root.join(".wikitool/remilia-parser.json"),
            project_root,
            root_source: crate::runtime::ValueSource::Flag,
            data_source: crate::runtime::ValueSource::Default,
            config_source: crate::runtime::ValueSource::Default,
        };
        (temp, paths)
    }

    #[test]
    fn migrations_apply_on_fresh_db() {
        let (_temp, paths) = test_paths();
        let report = run_migrations(&paths).expect("run_migrations");
        assert_eq!(report.applied.len(), MIGRATIONS.len());
        assert_eq!(report.current_version, 4);
    }

    #[test]
    fn migrations_are_idempotent() {
        let (_temp, paths) = test_paths();
        let first = run_migrations(&paths).expect("first run");
        assert_eq!(first.applied.len(), MIGRATIONS.len());

        let second = run_migrations(&paths).expect("second run");
        assert!(second.applied.is_empty());
        assert_eq!(second.current_version, 4);
    }

    #[test]
    fn pending_count_on_fresh_db() {
        let (_temp, paths) = test_paths();
        let count = pending_migration_count(&paths).expect("pending count");
        assert_eq!(count, MIGRATIONS.len());
    }

    #[test]
    fn pending_count_after_migration() {
        let (_temp, paths) = test_paths();
        run_migrations(&paths).expect("run_migrations");
        let count = pending_migration_count(&paths).expect("pending count");
        assert_eq!(count, 0);
    }
}
