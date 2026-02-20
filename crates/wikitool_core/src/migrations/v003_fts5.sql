-- v003: FTS5 full-text search virtual tables
-- Uses external-content mode to avoid data duplication.
-- After index rebuild, trigger: INSERT INTO <fts>(fts) VALUES('rebuild')

-- FTS5 for indexed_pages (search by title)
CREATE VIRTUAL TABLE IF NOT EXISTS indexed_pages_fts USING fts5(
    title,
    namespace,
    content=indexed_pages,
    content_rowid=rowid
);

-- FTS5 for extension_doc_pages (search by title + content)
CREATE VIRTUAL TABLE IF NOT EXISTS extension_doc_pages_fts USING fts5(
    page_title,
    content,
    content=extension_doc_pages,
    content_rowid=rowid
);

-- FTS5 for technical_docs (search by title + content)
CREATE VIRTUAL TABLE IF NOT EXISTS technical_docs_fts USING fts5(
    page_title,
    content,
    content=technical_docs,
    content_rowid=rowid
);
