use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use wikitool_core::filesystem::{ScanOptions, validate_scoped_path};
use wikitool_core::knowledge::authoring::AuthoringContractTraversalPlan;
use wikitool_core::knowledge::content_index::{RebuildReport, rebuild_index};
use wikitool_core::knowledge::status::{KnowledgeReadinessLevel, KnowledgeStatusReport};
use wikitool_core::runtime::{ResolvedPaths, ensure_runtime_ready_for_sync, inspect_runtime};

use crate::LOCAL_DB_POLICY_MESSAGE;
use crate::cli_support::{collapse_whitespace, format_flag, normalize_path};
pub(super) fn rebuild_knowledge_index(paths: &ResolvedPaths) -> Result<RebuildReport> {
    let status = inspect_runtime(paths)?;
    ensure_runtime_ready_for_sync(paths, &status)?;
    rebuild_index(paths, &ScanOptions::default())
}

pub(super) fn print_knowledge_status(prefix: &str, status: &KnowledgeStatusReport) {
    println!(
        "{prefix}.docs_profile_requested: {}",
        status.docs_profile_requested
    );
    println!(
        "{prefix}.readiness: {}",
        format_readiness(&status.readiness)
    );
    println!(
        "{prefix}.degradations: {}",
        format_list(&status.degradations)
    );
    println!(
        "{prefix}.knowledge_generation: {}",
        status.knowledge_generation
    );
    println!("{prefix}.db_exists: {}", format_flag(status.db_exists));
    println!(
        "{prefix}.content_index_ready: {}",
        format_flag(status.content_index_ready)
    );
    println!(
        "{prefix}.docs_profile_ready: {}",
        format_flag(status.docs_profile_ready)
    );
    println!("{prefix}.index_rows: {}", status.index_rows);
    println!(
        "{prefix}.docs_profile_corpora: {}",
        status.docs_profile_corpora
    );
    if let Some(artifact) = &status.content_index_artifact {
        println!(
            "{prefix}.content_index_artifact: key={} rows={} built_at_unix={}",
            artifact.artifact_key, artifact.row_count, artifact.built_at_unix
        );
    } else {
        println!("{prefix}.content_index_artifact: <missing>");
    }
    if let Some(artifact) = &status.docs_profile_artifact {
        println!(
            "{prefix}.docs_profile_artifact: key={} rows={} built_at_unix={}",
            artifact.artifact_key, artifact.row_count, artifact.built_at_unix
        );
    } else {
        println!("{prefix}.docs_profile_artifact: <missing>");
    }
}

pub(super) fn format_readiness(value: &KnowledgeReadinessLevel) -> &'static str {
    match value {
        KnowledgeReadinessLevel::NotReady => "not_ready",
        KnowledgeReadinessLevel::ContentReady => "content_ready",
        KnowledgeReadinessLevel::AuthoringReady => "authoring_ready",
    }
}

pub(super) fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}

pub(super) fn query_terms_for_contract_query(query: &str) -> Vec<String> {
    collapse_whitespace(query)
        .split_whitespace()
        .map(|term| {
            term.chars()
                .filter(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|term| term.chars().count() >= 2)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn print_contract_plan(
    label: &str,
    paths: &ResolvedPaths,
    plan: &AuthoringContractTraversalPlan,
) {
    println!("{label}");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {}", plan.query);
    println!("profile: {}", plan.profile.as_str());
    println!(
        "matched_query_terms: {}",
        format_list(&plan.matched_query_terms)
    );
    println!(
        "missing_query_terms: {}",
        format_list(&plan.missing_query_terms)
    );
    println!("token_budget: {}", plan.token_budget);
    println!("tokens_estimate_total: {}", plan.token_estimate_total);
    println!("selected.count: {}", plan.selected_contracts.len());
    for contract in &plan.selected_contracts {
        println!(
            "contract: kind={} title={} category={} score={} tokens={} usage={} params={} functions={} modules={}",
            contract.contract_kind,
            contract.title,
            contract.category,
            contract.score,
            contract.token_estimate,
            contract.usage_count,
            format_list(&contract.parameter_keys),
            format_list(&contract.function_names),
            format_list(&contract.module_titles)
        );
        for reason in &contract.selection_reasons {
            println!(
                "  reason: signal={} weight={} detail={} evidence={}",
                reason.signal,
                reason.weight,
                reason.detail,
                format_list(&reason.evidence_titles)
            );
        }
        for hint in &contract.expansion_hints {
            println!("  expand: {} ({})", hint.command, hint.reason);
        }
    }
    println!("edges.count: {}", plan.contract_edges.len());
    for edge in &plan.contract_edges {
        println!(
            "edge: {}:{} --{}--> {}:{}",
            edge.from_kind, edge.from_title, edge.relation, edge.to_kind, edge.to_title
        );
    }
    println!("omitted.count: {}", plan.omitted_contracts.len());
    for omission in &plan.omitted_contracts {
        println!(
            "omitted: kind={} title={} score={} reason={}",
            omission.contract_kind, omission.title, omission.score, omission.reason
        );
    }
    println!("warnings: {}", format_list(&plan.warnings));
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
}

pub(super) fn load_knowledge_stub_content(
    paths: &ResolvedPaths,
    stub_path: Option<&Path>,
) -> Result<Option<String>> {
    let Some(path) = stub_path else {
        return Ok(None);
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute)?;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read {}", normalize_path(&absolute)))?;
    Ok(Some(content))
}

pub(super) fn derive_topic_from_stub_path(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let stem = path.file_stem()?.to_string_lossy();
    let normalized = collapse_whitespace(&stem.replace('_', " "));
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::derive_topic_from_stub_path;
    use std::path::Path;

    #[test]
    fn derive_topic_from_stub_path_normalizes_filename() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/Remilia_Corporation.md"))),
            Some("Remilia Corporation".to_string())
        );
    }

    #[test]
    fn derive_topic_from_stub_path_rejects_blank_stem() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/___.md"))),
            None
        );
    }
}
