mod content;
mod siteinfo;
mod subpages;
mod templates;

pub use content::{fetch_mediawiki_page, fetch_pages_by_titles};
pub use subpages::list_subpages;
pub use templates::fetch_mediawiki_template_report;

use crate::research::model::ExternalFetchResult;

#[derive(Clone)]
pub(super) enum MediaWikiFetchOutcome {
    Found(Box<ExternalFetchResult>),
    Missing,
    NotExportable,
}
