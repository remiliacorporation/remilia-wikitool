-- v008: authoring-oriented index tables for AI retrieval and template implementation reference

CREATE TABLE IF NOT EXISTS indexed_page_aliases (
    alias_title TEXT PRIMARY KEY,
    canonical_title TEXT NOT NULL,
    canonical_namespace TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_aliases_canonical
    ON indexed_page_aliases(canonical_title);

CREATE TABLE IF NOT EXISTS indexed_page_sections (
    source_relative_path TEXT NOT NULL,
    section_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    section_level INTEGER NOT NULL,
    summary_text TEXT NOT NULL,
    section_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, section_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_sections_title
    ON indexed_page_sections(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_sections_heading
    ON indexed_page_sections(section_heading);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_sections_fts USING fts5(
    source_title,
    section_heading,
    summary_text,
    section_text,
    content=indexed_page_sections,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS indexed_template_examples (
    template_title TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    invocation_index INTEGER NOT NULL,
    example_wikitext TEXT NOT NULL,
    parameter_keys TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (template_title, source_relative_path, invocation_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_template_examples_template
    ON indexed_template_examples(template_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_examples_source
    ON indexed_template_examples(source_title);

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_template_examples_fts USING fts5(
    template_title,
    source_title,
    example_wikitext,
    content=indexed_template_examples,
    content_rowid=rowid
);

INSERT INTO indexed_page_sections_fts(indexed_page_sections_fts) VALUES('rebuild');
INSERT INTO indexed_template_examples_fts(indexed_template_examples_fts) VALUES('rebuild');
