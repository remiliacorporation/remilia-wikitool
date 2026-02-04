-- =============================================================================
-- WIKITOOL INITIAL SCHEMA
-- Version: 001
-- Description: Core tables for wiki content, sync state, and documentation
-- =============================================================================

-- =============================================================================
-- NAMESPACE CONSTANTS (MediaWiki standard)
-- =============================================================================
-- 0   = Main (articles)
-- 6   = File
-- 8   = MediaWiki (site CSS/JS)
-- 10  = Template
-- 14  = Category
-- 828 = Module (Scribunto)
-- 3000 = Goldenlight (custom)

-- =============================================================================
-- PAGES TABLE - Full content store for wiki pages
-- =============================================================================
CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE,

    -- Namespace handling
    namespace INTEGER NOT NULL DEFAULT 0,
    page_type TEXT NOT NULL,  -- article, template, module, mediawiki, category, redirect, file

    -- File tracking
    filename TEXT NOT NULL,
    filepath TEXT NOT NULL,           -- e.g., "wiki_content/Main/Charlotte_Fang.wiki"
    template_category TEXT,           -- For templates: infobox, cite, navbox, etc.

    -- FULL CONTENT (for LLM access and FTS)
    content TEXT,                     -- Full wikitext content
    content_hash TEXT,                -- SHA-256 for change detection

    -- Timestamps
    file_mtime INTEGER,               -- File modification time (Unix timestamp ms)
    wiki_modified_at TEXT,            -- Wiki revision timestamp (ISO 8601)
    last_synced_at TEXT,

    -- Sync state
    sync_status TEXT NOT NULL DEFAULT 'synced'
        CHECK (sync_status IN ('synced', 'local_modified', 'wiki_modified', 'conflict', 'staged', 'new')),
    is_redirect INTEGER DEFAULT 0,
    redirect_target TEXT,

    -- Metadata
    content_model TEXT,               -- wikitext, Scribunto, css, javascript
    page_id INTEGER,                  -- MediaWiki page ID
    revision_id INTEGER,              -- Latest revision ID

    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_pages_namespace ON pages(namespace);
CREATE INDEX IF NOT EXISTS idx_pages_sync_status ON pages(sync_status);
CREATE INDEX IF NOT EXISTS idx_pages_page_type ON pages(page_type);
CREATE INDEX IF NOT EXISTS idx_pages_filepath ON pages(filepath);
CREATE INDEX IF NOT EXISTS idx_pages_content_hash ON pages(content_hash);

-- =============================================================================
-- CATEGORIES
-- =============================================================================
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

-- =============================================================================
-- EXTENSION DOCUMENTATION (Tier 2)
-- =============================================================================
CREATE TABLE IF NOT EXISTS extension_docs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    extension_name TEXT NOT NULL UNIQUE,
    source_wiki TEXT NOT NULL DEFAULT 'mediawiki.org',
    version TEXT,
    pages_count INTEGER DEFAULT 0,
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT                   -- 7 days from fetch
);

CREATE TABLE IF NOT EXISTS extension_doc_pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    extension_id INTEGER NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT,                     -- Full content for FTS
    content_hash TEXT,
    fetched_at TEXT,
    FOREIGN KEY (extension_id) REFERENCES extension_docs(id) ON DELETE CASCADE,
    UNIQUE(extension_id, page_title)
);

-- =============================================================================
-- TECHNICAL DOCUMENTATION (Tier 3: Hooks, Config, API, etc.)
-- =============================================================================
CREATE TABLE IF NOT EXISTS technical_docs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_type TEXT NOT NULL,           -- 'hooks', 'config', 'api', 'manual'
    page_title TEXT NOT NULL,         -- e.g., "Manual:Hooks", "Manual:$wgServer"
    local_path TEXT NOT NULL,
    content TEXT,
    content_hash TEXT,
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT,                  -- 7 days from fetch
    UNIQUE(doc_type, page_title)
);

CREATE INDEX IF NOT EXISTS idx_technical_docs_type ON technical_docs(doc_type);

-- =============================================================================
-- UNIFIED FULL-TEXT SEARCH (single table, tier column)
-- =============================================================================
CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(
    tier,           -- 'content', 'extension', or 'technical'
    title,
    content,
    tokenize='porter unicode61 remove_diacritics 1'
);

-- =============================================================================
-- SYNC LOG (simple, no rollback data - wiki handles versioning)
-- =============================================================================
CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL CHECK (operation IN ('pull', 'push', 'delete', 'resolve', 'init')),
    page_title TEXT,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'conflict', 'skipped')),
    revision_id INTEGER,              -- Wiki revision after operation
    error_message TEXT,
    details TEXT,                     -- JSON blob
    timestamp TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sync_log_timestamp ON sync_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_sync_log_page ON sync_log(page_title);

-- =============================================================================
-- EXTERNAL FETCH CACHE
-- =============================================================================
CREATE TABLE IF NOT EXISTS fetch_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_wiki TEXT NOT NULL,        -- wikipedia, mediawiki, miraheze, fandom
    source_domain TEXT NOT NULL,
    page_title TEXT NOT NULL,
    content TEXT,
    content_format TEXT DEFAULT 'wikitext',
    fetched_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT,                  -- 24 hours from fetch
    UNIQUE(source_wiki, source_domain, page_title, content_format)
);

CREATE INDEX IF NOT EXISTS idx_fetch_cache_expires ON fetch_cache(expires_at);

-- =============================================================================
-- REFERENCE ARTICLES (permanent saves for structure comparison)
-- =============================================================================
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

-- =============================================================================
-- CONFIGURATION
-- =============================================================================
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT,
    updated_at TEXT DEFAULT (datetime('now'))
);

-- Insert default configuration values
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

-- =============================================================================
-- SCHEMA MIGRATIONS TABLE
-- =============================================================================
CREATE TABLE IF NOT EXISTS schema_migrations (
    version TEXT PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO schema_migrations (version) VALUES ('001');
