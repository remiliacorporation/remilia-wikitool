-- v009: citation/reference/media index tables for authoring-oriented retrieval

CREATE TABLE IF NOT EXISTS indexed_page_references (
    source_relative_path TEXT NOT NULL,
    reference_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    reference_name TEXT,
    reference_group TEXT,
    summary_text TEXT NOT NULL,
    reference_wikitext TEXT NOT NULL,
    template_titles TEXT NOT NULL,
    link_titles TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, reference_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_title
    ON indexed_page_references(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_name
    ON indexed_page_references(reference_name);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_references_fts USING fts5(
    source_title,
    section_heading,
    summary_text,
    reference_wikitext,
    template_titles,
    link_titles,
    content=indexed_page_references,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS indexed_page_media (
    source_relative_path TEXT NOT NULL,
    media_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    file_title TEXT NOT NULL,
    media_kind TEXT NOT NULL,
    caption_text TEXT NOT NULL,
    options_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, media_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_media_file
    ON indexed_page_media(file_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_media_title
    ON indexed_page_media(source_title);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_media_fts USING fts5(
    source_title,
    section_heading,
    file_title,
    caption_text,
    options_text,
    content=indexed_page_media,
    content_rowid=rowid
);

INSERT INTO indexed_page_references_fts(indexed_page_references_fts) VALUES('rebuild');
INSERT INTO indexed_page_media_fts(indexed_page_media_fts) VALUES('rebuild');
