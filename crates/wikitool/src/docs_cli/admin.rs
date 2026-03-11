use super::*;

#[derive(Debug, Args)]
pub(super) struct DocsListArgs {
    #[arg(long, help = "Show only outdated docs")]
    outdated: bool,
    #[arg(long, value_name = "TYPE", help = "Filter technical docs by type")]
    r#type: Option<String>,
    #[arg(long, value_name = "KIND", help = "Filter corpora by kind")]
    kind: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Filter corpora by source profile"
    )]
    profile: Option<String>,
}

pub(super) fn run_docs_list(runtime: &RuntimeOptions, args: DocsListArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let listing = list_docs(
        &paths,
        &DocsListOptions {
            technical_type: args.r#type.clone(),
            corpus_kind: args.kind.clone(),
            profile: args.profile.clone(),
        },
    )?;

    println!("docs list");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("stats.corpora_count: {}", listing.stats.corpora_count);
    println!("stats.pages_count: {}", listing.stats.pages_count);
    println!("stats.sections_count: {}", listing.stats.sections_count);
    println!("stats.symbols_count: {}", listing.stats.symbols_count);
    println!("stats.examples_count: {}", listing.stats.examples_count);
    for (kind, count) in &listing.stats.corpora_by_kind {
        println!("stats.corpora_by_kind.{kind}: {count}");
    }
    for (doc_type, count) in &listing.stats.technical_by_type {
        println!("stats.technical_by_type.{doc_type}: {count}");
    }

    if args.outdated {
        println!("outdated.corpora.count: {}", listing.outdated.corpora.len());
        for corpus in &listing.outdated.corpora {
            println!(
                "outdated.corpus: [{}] {} ({})",
                corpus.corpus_kind,
                corpus.label,
                format_expiration(listing.now_unix, corpus.expires_at_unix)
            );
        }
        return Ok(());
    }

    println!("corpora.count: {}", listing.corpora.len());
    for corpus in &listing.corpora {
        println!(
            "corpus: [{}] {} profile={} version={} pages={} sections={} symbols={} examples={} status={}",
            corpus.corpus_kind,
            corpus.label,
            if corpus.source_profile.is_empty() {
                "<none>"
            } else {
                &corpus.source_profile
            },
            if corpus.source_version.is_empty() {
                "<none>"
            } else {
                &corpus.source_version
            },
            corpus.pages_count,
            corpus.sections_count,
            corpus.symbols_count,
            corpus.examples_count,
            format_expiration(listing.now_unix, corpus.expires_at_unix)
        );
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(super) fn run_docs_update(runtime: &RuntimeOptions) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let report = update_outdated_docs_with_config(&paths, &config)?;

    println!("docs update");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("updated_corpora: {}", report.updated_corpora);
    println!("updated_pages: {}", report.updated_pages);
    println!("updated_sections: {}", report.updated_sections);
    println!("updated_symbols: {}", report.updated_symbols);
    println!("updated_examples: {}", report.updated_examples);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(super) fn run_docs_remove(runtime: &RuntimeOptions, target: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = remove_docs(&paths, target)?;

    println!("docs remove");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("target: {}", report.target);
    println!("kind: {:?}", report.kind);
    println!("removed_rows: {}", report.removed_rows);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if matches!(report.kind, DocsRemoveKind::NotFound) {
        bail!("documentation target not found: {target}");
    }
    Ok(())
}
