mod analysis;
mod extract;
mod identifiers;
mod signals;
mod source;
mod templates;

pub(crate) use extract::{extract_reference_records, extract_reference_records_from_sections};
pub(crate) use identifiers::{
    build_reference_authority_key, build_reference_authority_retrieval_text,
    normalize_reference_identifier_token, normalize_reference_identifier_value,
    parse_identifier_entries,
};
#[cfg(test)]
pub(crate) use source::extract_first_url;
pub(crate) use source::is_media_option;
