mod metadata;
mod model;
mod result;
mod tags;
mod text;

#[cfg(test)]
pub(super) use metadata::extract_html_metadata;
pub(super) use metadata::{derive_title_from_url, extract_client_redirect_url};
#[cfg(test)]
pub(super) use model::HtmlMetadata;
#[cfg(test)]
pub(super) use result::build_metadata_fallback_content;
pub(super) use result::{build_html_fetch_result, build_text_fetch_result};
pub(super) use tags::{decode_html, extract_head, index_of_ignore_case, scan_tags};
pub(crate) use text::truncate_to_byte_limit;
#[cfg(test)]
pub(super) use text::{
    collapse_inline_whitespace, detect_app_shell_html, extract_readable_text,
    normalize_extracted_text,
};
pub(super) use text::{
    detect_access_challenge, detect_access_challenge_vendor, read_text_body_limited,
};
