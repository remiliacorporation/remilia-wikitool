use super::import::collect_pages_for_technical_task;
use super::*;

pub(super) fn resolve_docs_profile(value: &str) -> Result<&'static DocsProfileDefinition> {
    let normalized = normalize_title(value);
    DOCS_PROFILES
        .iter()
        .find(|profile| profile.id.eq_ignore_ascii_case(&normalized))
        .ok_or_else(|| anyhow::anyhow!("unsupported docs profile `{normalized}`"))
}

pub(super) fn collect_profile_pages<A: DocsApi>(
    api: &mut A,
    definition: &DocsProfileDefinition,
    limit: usize,
) -> Result<Vec<FetchedDocsPage>> {
    let mut pages = Vec::new();
    let mut seen = BTreeSet::new();
    for seed in definition.page_seeds {
        let doc_type = infer_doc_type_from_title(seed.title);
        let mut task = TechnicalImportTask {
            doc_type,
            page_title: Some(seed.title.to_string()),
            include_subpages: seed.include_subpages,
        };
        let mut fetched = collect_pages_for_technical_task(api, &mut task, limit)?;
        for page in fetched.drain(..) {
            let key = page.page_title.to_ascii_lowercase();
            if seen.insert(key) {
                pages.push(page);
            }
        }
    }
    if pages.is_empty() {
        bail!("docs profile `{}` fetched no pages", definition.id);
    }
    Ok(pages)
}
