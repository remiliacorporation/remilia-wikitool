use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(in crate::source::web_fetch) struct TagMatch {
    pub(in crate::source::web_fetch) attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::source::web_fetch) struct HtmlMetadata {
    pub(in crate::source::web_fetch) title: Option<String>,
    pub(in crate::source::web_fetch) canonical_url: Option<String>,
    pub(in crate::source::web_fetch) site_name: Option<String>,
    pub(in crate::source::web_fetch) byline: Option<String>,
    pub(in crate::source::web_fetch) published_at: Option<String>,
    pub(in crate::source::web_fetch) description: Option<String>,
}
