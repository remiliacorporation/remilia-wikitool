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
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_lower_title
    ON sync_ledger_pages(lower(title));

CREATE TABLE IF NOT EXISTS sync_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

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
CREATE INDEX IF NOT EXISTS idx_indexed_pages_lower_title
    ON indexed_pages(lower(title));
CREATE INDEX IF NOT EXISTS idx_indexed_pages_ns_redirect
    ON indexed_pages(namespace, is_redirect);

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

CREATE VIRTUAL TABLE IF NOT EXISTS indexed_pages_fts USING fts5(
    title,
    namespace,
    content=indexed_pages,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS template_category_mappings (
    prefix TEXT PRIMARY KEY,
    category TEXT NOT NULL
);
INSERT OR IGNORE INTO template_category_mappings (prefix, category) VALUES
    ('Template:Cite', 'cite'),
    ('Module:Citation', 'cite'),
    ('Template:Ref', 'reference'),
    ('Template:Efn', 'reference'),
    ('Module:Reference', 'reference'),
    ('Template:Infobox', 'infobox'),
    ('Module:Infobox', 'infobox'),
    ('Module:InfoboxImage', 'infobox'),
    ('Template:About', 'hatnote'),
    ('Template:See also', 'hatnote'),
    ('Template:Main', 'hatnote'),
    ('Template:Further', 'hatnote'),
    ('Template:Hatnote', 'hatnote'),
    ('Template:Redirect', 'hatnote'),
    ('Template:Distinguish', 'hatnote'),
    ('Module:Hatnote', 'hatnote'),
    ('Template:Navbox', 'navbox'),
    ('Template:Navbar', 'navbox'),
    ('Template:Flatlist', 'navbox'),
    ('Template:Hlist', 'navbox'),
    ('Module:Navbox', 'navbox'),
    ('Module:Navbar', 'navbox'),
    ('Template:Blockquote', 'quotation'),
    ('Template:Cquote', 'quotation'),
    ('Template:Quote', 'quotation'),
    ('Template:Poem', 'quotation'),
    ('Template:Verse', 'quotation'),
    ('Module:Quotation', 'quotation'),
    ('Template:Ambox', 'message'),
    ('Template:Article quality', 'message'),
    ('Template:Stub', 'message'),
    ('Template:Update', 'message'),
    ('Template:Citation needed', 'message'),
    ('Template:Cn', 'message'),
    ('Template:Clarify', 'message'),
    ('Template:When', 'message'),
    ('Template:As of', 'message'),
    ('Module:Message', 'message'),
    ('Template:Sidebar', 'sidebar'),
    ('Template:Portal', 'sidebar'),
    ('Template:Remilia events', 'sidebar'),
    ('Module:Sidebar', 'sidebar'),
    ('Template:Repost', 'repost'),
    ('Template:Mirror', 'repost'),
    ('Template:Goldenlight repost', 'repost'),
    ('Module:Repost', 'repost'),
    ('Template:Etherscan', 'blockchain'),
    ('Template:Explorer', 'blockchain'),
    ('Template:OpenSea', 'blockchain'),
    ('Template:Translation', 'translations'),
    ('Module:Translation', 'translations'),
    ('Template:Birth date', 'date'),
    ('Template:Start date', 'date'),
    ('Template:End date', 'date'),
    ('Module:Age', 'date'),
    ('Template:Remilia navigation', 'navigation');

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

CREATE TABLE IF NOT EXISTS indexed_template_invocations (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    template_title TEXT NOT NULL,
    parameter_keys TEXT NOT NULL,
    PRIMARY KEY (source_relative_path, template_title, parameter_keys),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_template
    ON indexed_template_invocations(template_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_source
    ON indexed_template_invocations(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_lower_template
    ON indexed_template_invocations(lower(template_title));

CREATE TABLE IF NOT EXISTS indexed_page_aliases (
    alias_title TEXT PRIMARY KEY,
    canonical_title TEXT NOT NULL,
    canonical_namespace TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_aliases_canonical
    ON indexed_page_aliases(canonical_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_aliases_lower_alias
    ON indexed_page_aliases(lower(alias_title));

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

CREATE TABLE IF NOT EXISTS indexed_page_references (
    source_relative_path TEXT NOT NULL,
    reference_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    reference_name TEXT,
    reference_group TEXT,
    citation_profile TEXT NOT NULL,
    citation_family TEXT NOT NULL,
    primary_template_title TEXT NOT NULL,
    source_type TEXT NOT NULL,
    source_origin TEXT NOT NULL,
    reference_title TEXT NOT NULL,
    source_container TEXT NOT NULL,
    source_author TEXT NOT NULL,
    source_domain TEXT NOT NULL,
    identifier_keys TEXT NOT NULL,
    retrieval_signals TEXT NOT NULL,
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
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_profile
    ON indexed_page_references(citation_profile);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_type
    ON indexed_page_references(source_type);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_domain
    ON indexed_page_references(source_domain);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_template
    ON indexed_page_references(primary_template_title);
CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_references_fts USING fts5(
    source_title,
    section_heading,
    citation_profile,
    citation_family,
    source_type,
    reference_title,
    source_container,
    source_author,
    source_domain,
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

CREATE TABLE IF NOT EXISTS indexed_page_semantics (
    source_relative_path TEXT PRIMARY KEY,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    section_headings TEXT NOT NULL,
    category_titles TEXT NOT NULL,
    template_titles TEXT NOT NULL,
    template_parameter_keys TEXT NOT NULL,
    link_titles TEXT NOT NULL,
    reference_titles TEXT NOT NULL,
    reference_containers TEXT NOT NULL,
    reference_domains TEXT NOT NULL,
    media_titles TEXT NOT NULL,
    media_captions TEXT NOT NULL,
    semantic_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_semantics_title
    ON indexed_page_semantics(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_semantics_namespace
    ON indexed_page_semantics(source_namespace);
CREATE VIRTUAL TABLE IF NOT EXISTS indexed_page_semantics_fts USING fts5(
    source_title,
    summary_text,
    section_headings,
    category_titles,
    template_titles,
    template_parameter_keys,
    link_titles,
    reference_titles,
    reference_containers,
    reference_domains,
    media_titles,
    media_captions,
    semantic_text,
    content=indexed_page_semantics,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS extension_docs (
    extension_name TEXT PRIMARY KEY,
    source_wiki TEXT NOT NULL,
    version TEXT,
    pages_count INTEGER NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_extension_docs_expires
    ON extension_docs(expires_at_unix);

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
CREATE VIRTUAL TABLE IF NOT EXISTS extension_doc_pages_fts USING fts5(
    page_title,
    content,
    content=extension_doc_pages,
    content_rowid=rowid
);

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
CREATE INDEX IF NOT EXISTS idx_technical_docs_expires
    ON technical_docs(expires_at_unix);
CREATE VIRTUAL TABLE IF NOT EXISTS technical_docs_fts USING fts5(
    page_title,
    content,
    content=technical_docs,
    content_rowid=rowid
);
