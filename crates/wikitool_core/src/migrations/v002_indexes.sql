-- v002: performance indexes
-- Adds functional indexes for case-insensitive lookups and composite indexes
-- for common query patterns.

-- Case-insensitive title lookups on sync_ledger_pages (used in WHERE lower(title) queries)
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_lower_title
    ON sync_ledger_pages(lower(title));

-- Case-insensitive title lookups on indexed_pages (used in search queries)
CREATE INDEX IF NOT EXISTS idx_indexed_pages_lower_title
    ON indexed_pages(lower(title));

-- Composite index for namespace + redirect filtering (common in validation)
CREATE INDEX IF NOT EXISTS idx_indexed_pages_ns_redirect
    ON indexed_pages(namespace, is_redirect);

-- Expiration lookups on extension_docs (used in update/refresh)
CREATE INDEX IF NOT EXISTS idx_extension_docs_expires
    ON extension_docs(expires_at_unix);

-- Expiration lookups on technical_docs (used in update/refresh)
CREATE INDEX IF NOT EXISTS idx_technical_docs_expires
    ON technical_docs(expires_at_unix);
