-- v005: AI context cache for token-efficient local retrieval
-- Stores chunked article context and adds FTS5 index for snippet retrieval.

CREATE TABLE IF NOT EXISTS indexed_page_chunks (
    source_relative_path TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    chunk_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, chunk_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_indexed_page_chunks_title
    ON indexed_page_chunks(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_chunks_tokens
    ON indexed_page_chunks(token_estimate);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_chunks_fts USING fts5(
    source_title,
    section_heading,
    chunk_text,
    content=indexed_page_chunks,
    content_rowid=rowid
);

INSERT INTO indexed_page_chunks_fts(indexed_page_chunks_fts) VALUES('rebuild');
