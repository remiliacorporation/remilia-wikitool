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
    source_family TEXT NOT NULL,
    authority_kind TEXT NOT NULL,
    source_authority TEXT NOT NULL,
    reference_title TEXT NOT NULL,
    source_container TEXT NOT NULL,
    source_author TEXT NOT NULL,
    source_domain TEXT NOT NULL,
    source_date TEXT NOT NULL,
    canonical_url TEXT NOT NULL,
    identifier_keys TEXT NOT NULL,
    identifier_entries TEXT NOT NULL,
    source_urls TEXT NOT NULL,
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
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_family
    ON indexed_page_references(source_family);
CREATE INDEX IF NOT EXISTS idx_indexed_page_references_authority
    ON indexed_page_references(source_authority);
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
    source_family,
    authority_kind,
    source_authority,
    reference_title,
    source_container,
    source_author,
    source_domain,
    source_date,
    canonical_url,
    identifier_entries,
    source_urls,
    summary_text,
    reference_wikitext,
    template_titles,
    link_titles,
    content=indexed_page_references,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS indexed_reference_authorities (
    source_relative_path TEXT NOT NULL,
    reference_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    citation_profile TEXT NOT NULL,
    citation_family TEXT NOT NULL,
    source_type TEXT NOT NULL,
    source_origin TEXT NOT NULL,
    source_family TEXT NOT NULL,
    authority_kind TEXT NOT NULL,
    authority_key TEXT NOT NULL,
    authority_label TEXT NOT NULL,
    primary_template_title TEXT NOT NULL,
    source_domain TEXT NOT NULL,
    source_container TEXT NOT NULL,
    source_author TEXT NOT NULL,
    identifier_keys TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    retrieval_text TEXT NOT NULL,
    PRIMARY KEY (source_relative_path, reference_index, authority_key),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_authorities_key
    ON indexed_reference_authorities(authority_key);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_authorities_label
    ON indexed_reference_authorities(authority_label);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_authorities_family
    ON indexed_reference_authorities(source_family);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_authorities_source
    ON indexed_reference_authorities(source_title);
CREATE VIRTUAL TABLE IF NOT EXISTS indexed_reference_authorities_fts USING fts5(
    source_title,
    section_heading,
    citation_profile,
    citation_family,
    source_type,
    source_origin,
    source_family,
    authority_kind,
    authority_label,
    primary_template_title,
    source_domain,
    source_container,
    source_author,
    summary_text,
    retrieval_text,
    content=indexed_reference_authorities,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS indexed_reference_identifiers (
    source_relative_path TEXT NOT NULL,
    reference_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    citation_profile TEXT NOT NULL,
    citation_family TEXT NOT NULL,
    source_type TEXT NOT NULL,
    source_origin TEXT NOT NULL,
    source_family TEXT NOT NULL,
    authority_key TEXT NOT NULL,
    authority_label TEXT NOT NULL,
    identifier_key TEXT NOT NULL,
    identifier_value TEXT NOT NULL,
    normalized_value TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    PRIMARY KEY (source_relative_path, reference_index, identifier_key, normalized_value),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_identifiers_key
    ON indexed_reference_identifiers(identifier_key);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_identifiers_value
    ON indexed_reference_identifiers(normalized_value);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_identifiers_authority
    ON indexed_reference_identifiers(authority_key);
CREATE INDEX IF NOT EXISTS idx_indexed_reference_identifiers_source
    ON indexed_reference_identifiers(source_title);

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
    reference_source_families TEXT NOT NULL,
    reference_authorities TEXT NOT NULL,
    reference_identifiers TEXT NOT NULL,
    media_titles TEXT NOT NULL,
    media_captions TEXT NOT NULL,
    template_implementation_titles TEXT NOT NULL,
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
    reference_source_families,
    reference_authorities,
    reference_identifiers,
    media_titles,
    media_captions,
    template_implementation_titles,
    semantic_text,
    content=indexed_page_semantics,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS indexed_template_implementation_pages (
    template_title TEXT NOT NULL,
    implementation_page_title TEXT NOT NULL,
    implementation_namespace TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    role TEXT NOT NULL,
    PRIMARY KEY (template_title, implementation_page_title, role),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_template_implementation_pages_template
    ON indexed_template_implementation_pages(template_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_implementation_pages_role
    ON indexed_template_implementation_pages(role);

CREATE TABLE IF NOT EXISTS docs_corpora (
    corpus_id TEXT PRIMARY KEY,
    corpus_kind TEXT NOT NULL,
    label TEXT NOT NULL,
    source_wiki TEXT NOT NULL,
    source_version TEXT NOT NULL,
    source_profile TEXT NOT NULL,
    technical_type TEXT NOT NULL,
    refresh_kind TEXT NOT NULL,
    refresh_spec TEXT NOT NULL,
    pages_count INTEGER NOT NULL,
    sections_count INTEGER NOT NULL,
    symbols_count INTEGER NOT NULL,
    examples_count INTEGER NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_docs_corpora_kind
    ON docs_corpora(corpus_kind);
CREATE INDEX IF NOT EXISTS idx_docs_corpora_profile
    ON docs_corpora(source_profile);
CREATE INDEX IF NOT EXISTS idx_docs_corpora_type
    ON docs_corpora(technical_type);
CREATE INDEX IF NOT EXISTS idx_docs_corpora_expires
    ON docs_corpora(expires_at_unix);

CREATE TABLE IF NOT EXISTS docs_pages (
    corpus_id TEXT NOT NULL,
    page_title TEXT NOT NULL,
    normalized_title_key TEXT NOT NULL,
    page_namespace TEXT NOT NULL,
    doc_type TEXT NOT NULL,
    title_aliases TEXT NOT NULL,
    local_path TEXT NOT NULL,
    raw_content TEXT NOT NULL,
    normalized_content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    semantic_text TEXT NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (corpus_id, page_title),
    FOREIGN KEY (corpus_id) REFERENCES docs_corpora(corpus_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_docs_pages_title
    ON docs_pages(page_title);
CREATE INDEX IF NOT EXISTS idx_docs_pages_title_key
    ON docs_pages(normalized_title_key);
CREATE INDEX IF NOT EXISTS idx_docs_pages_namespace
    ON docs_pages(page_namespace);
CREATE INDEX IF NOT EXISTS idx_docs_pages_doc_type
    ON docs_pages(doc_type);
CREATE VIRTUAL TABLE IF NOT EXISTS docs_pages_fts USING fts5(
    page_title,
    title_aliases,
    summary_text,
    normalized_content,
    semantic_text,
    content=docs_pages,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS docs_sections (
    corpus_id TEXT NOT NULL,
    page_title TEXT NOT NULL,
    section_index INTEGER NOT NULL,
    section_level INTEGER NOT NULL,
    section_heading TEXT,
    summary_text TEXT NOT NULL,
    section_text TEXT NOT NULL,
    semantic_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (corpus_id, page_title, section_index),
    FOREIGN KEY (corpus_id, page_title) REFERENCES docs_pages(corpus_id, page_title) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_docs_sections_title
    ON docs_sections(page_title);
CREATE INDEX IF NOT EXISTS idx_docs_sections_heading
    ON docs_sections(section_heading);
CREATE VIRTUAL TABLE IF NOT EXISTS docs_sections_fts USING fts5(
    page_title,
    section_heading,
    summary_text,
    section_text,
    semantic_text,
    content=docs_sections,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS docs_symbols (
    corpus_id TEXT NOT NULL,
    page_title TEXT NOT NULL,
    symbol_index INTEGER NOT NULL,
    symbol_kind TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    normalized_symbol_key TEXT NOT NULL,
    aliases TEXT NOT NULL,
    section_heading TEXT,
    signature_text TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    detail_text TEXT NOT NULL,
    retrieval_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (corpus_id, page_title, symbol_index),
    FOREIGN KEY (corpus_id, page_title) REFERENCES docs_pages(corpus_id, page_title) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_docs_symbols_name
    ON docs_symbols(symbol_name);
CREATE INDEX IF NOT EXISTS idx_docs_symbols_key
    ON docs_symbols(normalized_symbol_key);
CREATE INDEX IF NOT EXISTS idx_docs_symbols_kind
    ON docs_symbols(symbol_kind);
CREATE VIRTUAL TABLE IF NOT EXISTS docs_symbols_fts USING fts5(
    page_title,
    section_heading,
    symbol_kind,
    symbol_name,
    aliases,
    signature_text,
    summary_text,
    detail_text,
    retrieval_text,
    content=docs_symbols,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS docs_examples (
    corpus_id TEXT NOT NULL,
    page_title TEXT NOT NULL,
    example_index INTEGER NOT NULL,
    example_kind TEXT NOT NULL,
    section_heading TEXT,
    language_hint TEXT NOT NULL,
    summary_text TEXT NOT NULL,
    example_text TEXT NOT NULL,
    retrieval_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (corpus_id, page_title, example_index),
    FOREIGN KEY (corpus_id, page_title) REFERENCES docs_pages(corpus_id, page_title) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_docs_examples_title
    ON docs_examples(page_title);
CREATE INDEX IF NOT EXISTS idx_docs_examples_kind
    ON docs_examples(example_kind);
CREATE INDEX IF NOT EXISTS idx_docs_examples_language
    ON docs_examples(language_hint);
CREATE VIRTUAL TABLE IF NOT EXISTS docs_examples_fts USING fts5(
    page_title,
    section_heading,
    example_kind,
    language_hint,
    summary_text,
    example_text,
    retrieval_text,
    content=docs_examples,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS docs_links (
    corpus_id TEXT NOT NULL,
    page_title TEXT NOT NULL,
    link_index INTEGER NOT NULL,
    target_title TEXT NOT NULL,
    relation_kind TEXT NOT NULL,
    display_text TEXT NOT NULL,
    PRIMARY KEY (corpus_id, page_title, link_index),
    FOREIGN KEY (corpus_id, page_title) REFERENCES docs_pages(corpus_id, page_title) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_docs_links_target
    ON docs_links(target_title);
CREATE INDEX IF NOT EXISTS idx_docs_links_kind
    ON docs_links(relation_kind);
