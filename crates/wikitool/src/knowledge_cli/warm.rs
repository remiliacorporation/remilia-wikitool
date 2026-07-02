use anyhow::Result;
use wikitool_core::docs::{
    DocsImportProfileOptions, DocsImportProfileReport, import_docs_profile_with_config,
    is_transient_docs_error,
};
use wikitool_core::knowledge::status::knowledge_status;
use wikitool_core::runtime::ResolvedPaths;

use crate::cli_support::{
    normalize_path, print_database_schema_status, print_scan_stats, resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::{print_knowledge_status, rebuild_knowledge_index};
use super::*;
pub(crate) fn run_knowledge_warm(runtime: &RuntimeOptions, args: KnowledgeWarmArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let rebuild = rebuild_knowledge_index(&paths)?;
    let (docs_action, docs) = warm_docs_profile(&paths, &config, &args)?;
    let status = knowledge_status(&paths, &args.docs_profile)?;
    let report = KnowledgeWarmReport {
        rebuild,
        docs_action,
        docs,
        status,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge warm");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "docs_profile_requested: {}",
        report.status.docs_profile_requested
    );
    println!(
        "knowledge_generation: {}",
        report.status.knowledge_generation
    );
    println!("rebuild.unchanged: {}", report.rebuild.unchanged);
    println!("rebuild.inserted_rows: {}", report.rebuild.inserted_rows);
    println!("rebuild.inserted_links: {}", report.rebuild.inserted_links);
    println!("docs.action: {}", report.docs_action);
    println!("docs.imported_corpora: {}", report.docs.imported_corpora);
    println!("docs.imported_pages: {}", report.docs.imported_pages);
    println!("docs.failures.count: {}", report.docs.failures.len());
    for failure in &report.docs.failures {
        println!("docs.failure: {failure}");
    }
    print_scan_stats("scan", &report.rebuild.scan);
    print_knowledge_status("knowledge", &report.status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn warm_docs_profile(
    paths: &ResolvedPaths,
    config: &wikitool_core::config::WikiConfig,
    args: &KnowledgeWarmArgs,
) -> Result<(&'static str, DocsImportProfileReport)> {
    if matches!(args.docs_mode, KnowledgeWarmDocsMode::Skip) {
        return Ok((
            "skipped_requested",
            skipped_docs_profile_report(&args.docs_profile),
        ));
    }
    if matches!(args.docs_mode, KnowledgeWarmDocsMode::Missing)
        && knowledge_status(paths, &args.docs_profile)
            .map(|status| status.docs_profile_ready)
            .unwrap_or(false)
    {
        return Ok((
            "skipped_existing",
            skipped_docs_profile_report(&args.docs_profile),
        ));
    }

    match import_docs_profile_with_config(
        paths,
        &DocsImportProfileOptions {
            profile: args.docs_profile.clone(),
            ..DocsImportProfileOptions::default()
        },
        config,
    ) {
        Ok(report) => Ok(("refreshed", report)),
        Err(error) if is_transient_docs_error(&error) => Ok((
            "transient_failure",
            transient_docs_profile_report(&args.docs_profile, &error),
        )),
        Err(error) => Err(error),
    }
}

fn skipped_docs_profile_report(profile: &str) -> DocsImportProfileReport {
    DocsImportProfileReport {
        profile: profile.to_string(),
        imported_corpora: 0,
        imported_extensions: 0,
        imported_pages: 0,
        imported_sections: 0,
        imported_symbols: 0,
        imported_examples: 0,
        failures: Vec::new(),
        request_count: 0,
    }
}

fn transient_docs_profile_report(profile: &str, error: &anyhow::Error) -> DocsImportProfileReport {
    DocsImportProfileReport {
        profile: profile.to_string(),
        imported_corpora: 0,
        imported_extensions: 0,
        imported_pages: 0,
        imported_sections: 0,
        imported_symbols: 0,
        imported_examples: 0,
        failures: vec![format!("{error:#}")],
        request_count: 0,
    }
}
