/**
 * Database schema definitions
 *
 * The schema is embedded directly in code to avoid file path issues
 * across different build and runtime environments.
 */

/** Current schema version */
export const SCHEMA_VERSION = '005';

/** Initial schema SQL */
export const SCHEMA_001 = `
-- WIKITOOL INITIAL SCHEMA v001

-- PAGES TABLE - Full content store for wiki pages
CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE,
    namespace INTEGER NOT NULL DEFAULT 0,
    page_type TEXT NOT NULL,
    filename TEXT NOT NULL,
    filepath TEXT NOT NULL,
    template_category TEXT,
    content TEXT,
    content_hash TEXT,
    file_mtime INTEGER,
    wiki_modified_at TEXT,
    last_synced_at TEXT,
    sync_status TEXT NOT NULL DEFAULT 'synced'
        CHECK (sync_status IN ('synced', 'local_modified', 'wiki_modified', 'conflict', 'staged', 'new')),
    is_redirect INTEGER DEFAULT 0,
    redirect_target TEXT,
    content_model TEXT,
    page_id INTEGER,
    revision_id INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_pages_namespace ON pages(namespace);
CREATE INDEX IF NOT EXISTS idx_pages_sync_status ON pages(sync_status);
CREATE INDEX IF NOT EXISTS idx_pages_page_type ON pages(page_type);
CREATE INDEX IF NOT EXISTS idx_pages_filepath ON pages(filepath);
CREATE INDEX IF NOT EXISTS idx_pages_content_hash ON pages(content_hash);

-- CATEGORIES
CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    parent_category TEXT,
    page_count INTEGER DEFAULT 0,
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS page_categories (
    page_id INTEGER NOT NULL,
    category_id INTEGER NOT NULL,
    PRIMARY KEY (page_id, category_id),
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE,
    FOREIGN KEY (category_id) REFERENCES categories(id) ON DELETE CASCADE
);

-- EXTENSION DOCUMENTATION (Tier 2)
CREATE TABLE IF NOT EXISTS extension_docs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    extension_name TEXT NOT NULL UNIQUE,
    source_wiki TEXT NOT NULL DEFAULT 'mediawiki.org',
    version TEXT,
    pages_count INTEGER DEFAULT 0,
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT
);

CREATE TABLE IF NOT EXISTS extension_doc_pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    extension_id INTEGER NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT,
    content_hash TEXT,
    fetched_at TEXT,
    FOREIGN KEY (extension_id) REFERENCES extension_docs(id) ON DELETE CASCADE,
    UNIQUE(extension_id, page_title)
);

-- TECHNICAL DOCUMENTATION (Tier 3)
CREATE TABLE IF NOT EXISTS technical_docs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_type TEXT NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT,
    content_hash TEXT,
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT,
    UNIQUE(doc_type, page_title)
);

CREATE INDEX IF NOT EXISTS idx_technical_docs_type ON technical_docs(doc_type);

-- UNIFIED FULL-TEXT SEARCH
CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(
    tier,
    title,
    content,
    tokenize='porter unicode61 remove_diacritics 1'
);

-- SYNC LOG
CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL CHECK (operation IN ('pull', 'push', 'delete', 'resolve', 'init')),
    page_title TEXT,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'conflict', 'skipped')),
    revision_id INTEGER,
    error_message TEXT,
    details TEXT,
    timestamp TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sync_log_timestamp ON sync_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_sync_log_page ON sync_log(page_title);

-- EXTERNAL FETCH CACHE
CREATE TABLE IF NOT EXISTS fetch_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_wiki TEXT NOT NULL,
    source_domain TEXT NOT NULL,
    page_title TEXT NOT NULL,
    content TEXT,
    content_format TEXT DEFAULT 'wikitext',
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT,
    UNIQUE(source_wiki, source_domain, page_title, content_format)
);

CREATE INDEX IF NOT EXISTS idx_fetch_cache_expires ON fetch_cache(expires_at);

-- REFERENCE ARTICLES
CREATE TABLE IF NOT EXISTS reference_articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_wiki TEXT NOT NULL,
    source_domain TEXT NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT NOT NULL,
    local_name TEXT NOT NULL,
    filepath TEXT NOT NULL,
    content TEXT,
    content_hash TEXT,
    fetched_at TEXT DEFAULT (datetime('now')),
    notes TEXT,
    UNIQUE(source_wiki, source_domain, title)
);

-- CONFIGURATION
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT,
    updated_at TEXT DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO config (key, value) VALUES
    ('schema_version', '001'),
    ('last_article_pull', NULL),
    ('last_template_pull', NULL),
    ('wiki_api_url', 'https://wiki.remilia.org/api.php'),
    ('wiki_url', 'https://wiki.remilia.org'),
    ('rate_limit_read_ms', '300'),
    ('rate_limit_write_ms', '1000'),
    ('batch_size_read', '500'),
    ('batch_size_write', '50'),
    ('fetch_cache_ttl_hours', '24'),
    ('docs_cache_ttl_days', '7');

-- SCHEMA MIGRATIONS TABLE
CREATE TABLE IF NOT EXISTS schema_migrations (
    version TEXT PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO schema_migrations (version) VALUES ('001');
`;

/** Schema migration 002: Extended metadata and cache improvements */
export const SCHEMA_002 = `
-- Schema migration 002: Extended page metadata and fetch cache improvements
-- This migration adds columns to existing tables for parser/metadata features

-- Add metadata columns to pages table (safe ALTERs - SQLite handles existing columns gracefully)
ALTER TABLE pages ADD COLUMN shortdesc TEXT;
ALTER TABLE pages ADD COLUMN display_title TEXT;
ALTER TABLE pages ADD COLUMN word_count INTEGER;

-- Add category and tags to fetch_cache for better organization
ALTER TABLE fetch_cache ADD COLUMN category TEXT;
ALTER TABLE fetch_cache ADD COLUMN tags TEXT;

-- Create index for fetch_cache category
CREATE INDEX IF NOT EXISTS idx_fetch_cache_category ON fetch_cache(category);

-- NOTE: FTS rebuild may be needed after this migration if content structure changes
-- Run: wikitool index rebuild after initial contracts/bootstrap setup
`;

/** Schema migration 003: Link graph and template usage */
export const SCHEMA_003 = `
-- Schema migration 003: Link graph and template usage
-- NOTE: categories already exist in schema v001 (categories + page_categories)

-- Page links (derived from parsing)
CREATE TABLE IF NOT EXISTS page_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_page_id INTEGER NOT NULL,
    target_title TEXT NOT NULL,
    link_type TEXT NOT NULL DEFAULT 'internal',
    target_namespace INTEGER,
    FOREIGN KEY (source_page_id) REFERENCES pages(id) ON DELETE CASCADE,
    UNIQUE(source_page_id, target_title, link_type)
);

CREATE INDEX IF NOT EXISTS idx_page_links_source ON page_links(source_page_id);
CREATE INDEX IF NOT EXISTS idx_page_links_target ON page_links(target_title);

-- Template usage tracking
CREATE TABLE IF NOT EXISTS template_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    template_name TEXT NOT NULL,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_template_usage_page ON template_usage(page_id);
CREATE INDEX IF NOT EXISTS idx_template_usage_template ON template_usage(template_name);

-- Redirect mapping (for quick lookup)
CREATE TABLE IF NOT EXISTS redirects (
    source_title TEXT PRIMARY KEY,
    target_title TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_redirects_target ON redirects(target_title);
`;

/** Schema migration 004: Context layer (sections, template params, infoboxes, metadata, module deps) */
export const SCHEMA_004 = `
-- Schema migration 004: Context layer for AI retrieval

-- Page sections (lead + headings)
CREATE TABLE IF NOT EXISTS page_sections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    section_index INTEGER NOT NULL,
    heading TEXT,
    level INTEGER,
    anchor TEXT,
    content TEXT,
    is_lead INTEGER DEFAULT 0,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE,
    UNIQUE(page_id, section_index)
);

CREATE INDEX IF NOT EXISTS idx_page_sections_page ON page_sections(page_id);
CREATE INDEX IF NOT EXISTS idx_page_sections_heading ON page_sections(heading);

-- Section-level full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS page_sections_fts USING fts5(
    title,
    heading,
    content,
    page_id UNINDEXED,
    section_index UNINDEXED,
    tokenize='porter unicode61 remove_diacritics 1'
);

-- Template calls (ordered) and parameters
CREATE TABLE IF NOT EXISTS template_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    template_name TEXT NOT NULL,
    call_index INTEGER NOT NULL,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_template_calls_page ON template_calls(page_id);
CREATE INDEX IF NOT EXISTS idx_template_calls_template ON template_calls(template_name);

CREATE TABLE IF NOT EXISTS template_params (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    call_id INTEGER NOT NULL,
    param_index INTEGER NOT NULL,
    param_name TEXT,
    param_value TEXT,
    is_named INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (call_id) REFERENCES template_calls(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_template_params_call ON template_params(call_id);
CREATE INDEX IF NOT EXISTS idx_template_params_name ON template_params(param_name);

-- Infobox key/values (derived from template calls)
CREATE TABLE IF NOT EXISTS infobox_kv (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    infobox_name TEXT NOT NULL,
    param_name TEXT NOT NULL,
    param_value TEXT,
    call_index INTEGER,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_infobox_page ON infobox_kv(page_id);
CREATE INDEX IF NOT EXISTS idx_infobox_name ON infobox_kv(infobox_name);
CREATE INDEX IF NOT EXISTS idx_infobox_param ON infobox_kv(param_name);

-- Template metadata (TemplateData)
CREATE TABLE IF NOT EXISTS template_metadata (
    template_name TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    param_defs TEXT,
    description TEXT,
    example TEXT,
    updated_at TEXT DEFAULT (datetime('now'))
);

-- Module dependencies (Lua require/loadData)
CREATE TABLE IF NOT EXISTS module_deps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_title TEXT NOT NULL,
    dependency TEXT NOT NULL,
    dep_type TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_module_deps_module ON module_deps(module_title);
CREATE INDEX IF NOT EXISTS idx_module_deps_dep ON module_deps(dependency);
`;

/** Schema migration 005: Cargo tables */
export const SCHEMA_005 = `
-- Schema migration 005: Cargo tables

-- Cargo table declarations (from #cargo_declare)
CREATE TABLE IF NOT EXISTS cargo_tables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    table_name TEXT NOT NULL,
    columns TEXT NOT NULL,
    declare_raw TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE,
    UNIQUE(page_id, table_name)
);

-- Cargo data rows (from #cargo_store)
CREATE TABLE IF NOT EXISTS cargo_stores (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    table_name TEXT NOT NULL,
    values_json TEXT NOT NULL,
    store_raw TEXT,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE
);

-- Cargo queries (from #cargo_query)
CREATE TABLE IF NOT EXISTS cargo_queries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id INTEGER NOT NULL,
    query_type TEXT NOT NULL,
    tables TEXT NOT NULL,
    fields TEXT,
    params_json TEXT NOT NULL,
    query_raw TEXT,
    FOREIGN KEY (page_id) REFERENCES pages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cargo_tables_name ON cargo_tables(table_name);
CREATE INDEX IF NOT EXISTS idx_cargo_tables_page ON cargo_tables(page_id);
CREATE INDEX IF NOT EXISTS idx_cargo_stores_table ON cargo_stores(table_name);
CREATE INDEX IF NOT EXISTS idx_cargo_stores_page ON cargo_stores(page_id);
CREATE INDEX IF NOT EXISTS idx_cargo_queries_page ON cargo_queries(page_id);
`;

/** All migrations in order */
export const MIGRATIONS: { version: string; sql: string }[] = [
  { version: '001', sql: SCHEMA_001 },
  { version: '002', sql: SCHEMA_002 },
  { version: '003', sql: SCHEMA_003 },
  { version: '004', sql: SCHEMA_004 },
  { version: '005', sql: SCHEMA_005 },
];
