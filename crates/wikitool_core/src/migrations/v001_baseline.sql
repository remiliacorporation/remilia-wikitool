-- v001: baseline schema
-- Consolidates all existing CREATE TABLE/INDEX IF NOT EXISTS statements
-- from sync.rs, index.rs, and docs.rs into a single idempotent baseline.

-- === sync domain ===

CREATE TABLE IF NOT EXISTS sync_ledger_pages (
    title TEXT PRIMARY KEY,
    namespace INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    wiki_modified_at TEXT,
    revision_id INTEGER,
    page_id INTEGER,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    last_synced_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_namespace ON sync_ledger_pages(namespace);
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_relative_path ON sync_ledger_pages(relative_path);

CREATE TABLE IF NOT EXISTS sync_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- === index domain ===

CREATE TABLE IF NOT EXISTS indexed_pages (
    relative_path TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    namespace TEXT NOT NULL,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    content_hash TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    indexed_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_title ON indexed_pages(title);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_namespace ON indexed_pages(namespace);

CREATE TABLE IF NOT EXISTS indexed_links (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    target_title TEXT NOT NULL,
    target_namespace TEXT NOT NULL,
    is_category_membership INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, target_title, is_category_membership),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_links_target ON indexed_links(target_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_source ON indexed_links(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_category_membership ON indexed_links(is_category_membership, target_title);

-- === docs domain ===

CREATE TABLE IF NOT EXISTS extension_docs (
    extension_name TEXT PRIMARY KEY,
    source_wiki TEXT NOT NULL,
    version TEXT,
    pages_count INTEGER NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS extension_doc_pages (
    extension_name TEXT NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    PRIMARY KEY (extension_name, page_title),
    FOREIGN KEY (extension_name) REFERENCES extension_docs(extension_name) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_extension_doc_pages_title ON extension_doc_pages(page_title);

CREATE TABLE IF NOT EXISTS technical_docs (
    doc_type TEXT NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL,
    PRIMARY KEY (doc_type, page_title)
);
CREATE INDEX IF NOT EXISTS idx_technical_docs_type ON technical_docs(doc_type);
CREATE INDEX IF NOT EXISTS idx_technical_docs_title ON technical_docs(page_title);
