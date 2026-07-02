pub mod auth;
pub mod cargo_query;
pub mod client;
pub mod namespace;
pub mod read;
pub mod render;
pub mod search;
pub mod siteinfo;
pub mod write;

pub use cargo_query::cargo_count_rows;
pub use client::{
    ExternalSearchHit, MediaWikiClient, MediaWikiClientConfig, PageTimestampInfo, RemotePage,
    WikiReadApi, WikiWriteApi,
};
pub use namespace::{NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE};
pub use search::{
    ExternalSearchReport, MediaWikiSearchOptions, MediaWikiSearchWhat, search_pages_report,
};
pub use write::{
    AppliedProtection, MovePageOptions, MoveReport, ProtectPageOptions, ProtectReport,
    PurgeOptions, PurgePageReport, PurgeReport, UndeletePageOptions, UndeleteReport, UploadOptions,
    UploadReport,
};
