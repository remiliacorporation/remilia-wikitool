pub mod auth;
pub mod cargo_query;
pub mod client;
pub mod entities;
pub mod namespace;
pub mod read;
pub mod render;
pub mod render_assertions;
pub mod search;
pub mod siteinfo;
pub mod write;

pub use cargo_query::{
    CargoField, CargoRowsOptions, cargo_count_rows, cargo_list_tables, cargo_query_rows,
    cargo_table_fields,
};
pub use client::{
    DeleteLogEntry, DeleteOutcome, DeleteReceipt, EditConstraint, EditReceipt, ExternalSearchHit,
    MediaWikiClient, MediaWikiClientConfig, PageTimestampInfo, RemotePage, RevisionLineageEntry,
    WikiReadApi, WikiWriteApi,
};
pub use entities::decode_html_entities;
pub use namespace::{NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE};
pub use render::{
    MAX_RENDER_WIKITEXT_BYTES, RenderCheckIssue, RenderCheckOptions, RenderCheckReport,
    RenderedScopeReport, render_check_page, render_check_wikitext, render_wikitext_html,
};
pub use render_assertions::{
    RenderDomAssertion, RenderDomAssertionResult, validate_dom_assertions,
};
pub use search::{
    ExternalSearchReport, MediaWikiSearchOptions, MediaWikiSearchWhat, search_pages_report,
};
pub use write::{PurgeOptions, PurgePageReport, PurgeReport};
