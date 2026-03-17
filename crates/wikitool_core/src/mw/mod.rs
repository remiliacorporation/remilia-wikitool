pub mod auth;
pub mod client;
pub mod namespace;
pub mod read;
pub mod render;
pub mod search;
pub mod siteinfo;
pub mod write;

pub use client::{
    ExternalSearchHit, MediaWikiClient, MediaWikiClientConfig, PageTimestampInfo, RemotePage,
    WikiReadApi, WikiWriteApi,
};
pub use namespace::{NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE};
