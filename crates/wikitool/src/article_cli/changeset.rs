use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use wikitool_core::article_acceptance::ArticlePublicationAuthority;
use wikitool_core::article_changeset::{
    ArticleReviewChangeset, ArticleReviewChangesetInput, accept_article_review_changeset,
    prepare_article_review_changeset,
};
use wikitool_core::filesystem::relative_path_to_title;

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::selection::{
    article_selection_from_args, resolve_article_targets, single_state_path_title_override,
};
use super::*;

#[derive(Debug, Serialize)]
struct ArticleChangesetPrepareReport {
    schema_version: &'static str,
    project_root: String,
    manifest_path: String,
    item_count: usize,
    total_errors: usize,
    total_warnings: usize,
    total_suggestions: usize,
    manifest: ArticleReviewChangeset,
}

#[derive(Debug, Serialize)]
struct ArticleChangesetAcceptedItemReport {
    title: String,
    target_relative_path: String,
    content_sha256: String,
    prose_origin: String,
    editor_identity_assurance: String,
    warning_decision: String,
}

#[derive(Debug, Serialize)]
struct ArticleChangesetAcceptReport {
    schema_version: &'static str,
    project_root: String,
    manifest_path: String,
    changeset_sha256: String,
    decision_id: String,
    acceptance_store_path: String,
    human_editor_claim: String,
    editor_identity_assurance: String,
    warning_policy: String,
    accepted_at_unix: u64,
    publication_authority: ArticlePublicationAuthority,
    item_count: usize,
    items: Vec<ArticleChangesetAcceptedItemReport>,
}

pub(super) fn run_article_changeset_prepare(
    runtime: &RuntimeOptions,
    args: ArticleChangesetPrepareArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let prose_origin = args.prose_origin.into();
    let inputs = if let Some(title_override) = single_state_path_title_override(
        &paths,
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )? {
        vec![ArticleReviewChangesetInput {
            source_path: args.path.clone().expect("single state-draft path"),
            title: title_override.to_string(),
            prose_origin,
        }]
    } else {
        let selection = article_selection_from_args(
            &args.titles,
            &args.paths,
            args.titles_file.as_ref(),
            args.changed,
        )?;
        resolve_article_targets(&paths, args.path.as_deref(), &selection, false)?
            .into_iter()
            .map(|relative_path| {
                let title = relative_path_to_title(&paths, &relative_path)?;
                Ok(ArticleReviewChangesetInput {
                    source_path: PathBuf::from(relative_path),
                    title,
                    prose_origin,
                })
            })
            .collect::<Result<Vec<_>>>()?
    };

    let manifest = prepare_article_review_changeset(&paths, &args.output, inputs, args.replace)?;
    let report = ArticleChangesetPrepareReport {
        schema_version: "article_changeset_prepare_v2",
        project_root: normalize_path(&paths.project_root),
        manifest_path: normalize_path(absolute_path(&paths.project_root, &args.output)),
        item_count: manifest.items.len(),
        total_errors: manifest.items.iter().map(|item| item.lint.errors).sum(),
        total_warnings: manifest.items.iter().map(|item| item.lint.warnings).sum(),
        total_suggestions: manifest
            .items
            .iter()
            .map(|item| item.lint.suggestions)
            .sum(),
        manifest,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("article changeset prepare");
        println!("schema_version: {}", report.schema_version);
        println!("project_root: {}", report.project_root);
        println!("manifest_path: {}", report.manifest_path);
        println!("changeset_sha256: {}", report.manifest.changeset_sha256);
        println!("item_count: {}", report.item_count);
        println!("total_errors: {}", report.total_errors);
        println!("total_warnings: {}", report.total_warnings);
        println!("total_suggestions: {}", report.total_suggestions);
        for item in &report.manifest.items {
            println!(
                "item: title={} source={} target={} content_sha256={} prose_origin={} errors={} warnings={} suggestions={}",
                item.title,
                item.source_relative_path,
                item.target_relative_path,
                item.content_sha256,
                item.prose_origin.as_str(),
                item.lint.errors,
                item.lint.warnings,
                item.lint.suggestions
            );
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        println!(
            "next: a named human reads every exact item and lint finding, then runs `wikitool article changeset accept`"
        );
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}

pub(super) fn run_article_changeset_accept(
    runtime: &RuntimeOptions,
    args: ArticleChangesetAcceptArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let accepted = accept_article_review_changeset(
        &paths,
        &args.manifest,
        &args.human_editor,
        args.warnings.into(),
    )?;
    let items = accepted
        .decision
        .items
        .iter()
        .map(|item| ArticleChangesetAcceptedItemReport {
            title: item.review_item.title.clone(),
            target_relative_path: item.review_item.target_relative_path.clone(),
            content_sha256: item.review_item.content_sha256.clone(),
            prose_origin: item.review_item.prose_origin.as_str().to_string(),
            editor_identity_assurance: accepted.decision.editor_identity_assurance.clone(),
            warning_decision: item.warning_decision.as_str().to_string(),
        })
        .collect::<Vec<_>>();
    let report = ArticleChangesetAcceptReport {
        schema_version: "article_changeset_accept_v2",
        project_root: normalize_path(&paths.project_root),
        manifest_path: normalize_path(absolute_path(&paths.project_root, &args.manifest)),
        changeset_sha256: accepted.decision.changeset_sha256,
        decision_id: accepted.decision.decision_id,
        acceptance_store_path: normalize_path(&accepted.acceptance_store_path),
        human_editor_claim: accepted.decision.human_editor_claim,
        editor_identity_assurance: accepted.decision.editor_identity_assurance,
        warning_policy: accepted.decision.warning_policy.as_str().to_string(),
        accepted_at_unix: accepted.decision.accepted_at_unix,
        publication_authority: accepted.decision.publication_authority,
        item_count: items.len(),
        items,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("article changeset accept");
        println!("schema_version: {}", report.schema_version);
        println!("project_root: {}", report.project_root);
        println!("manifest_path: {}", report.manifest_path);
        println!("changeset_sha256: {}", report.changeset_sha256);
        println!("decision_id: {}", report.decision_id);
        println!("acceptance_store_path: {}", report.acceptance_store_path);
        println!("human_editor_claim: {}", report.human_editor_claim);
        println!(
            "editor_identity_assurance: {}",
            report.editor_identity_assurance
        );
        println!("warning_policy: {}", report.warning_policy);
        println!("accepted_at_unix: {}", report.accepted_at_unix);
        println!(
            "target_api_url: {}",
            report.publication_authority.target_api_url
        );
        println!(
            "site_adapter_id: {}",
            report.publication_authority.site_adapter_id
        );
        println!(
            "publication_policy_sha256: {}",
            report.publication_authority.publication_policy_sha256
        );
        println!("item_count: {}", report.item_count);
        for item in &report.items {
            println!(
                "item: title={} target={} content_sha256={} prose_origin={} warning_decision={}",
                item.title,
                item.target_relative_path,
                item.content_sha256,
                item.prose_origin,
                item.warning_decision
            );
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        println!(
            "ledger_scope: one exact-content decision with one independently invalidating ledger record per article; editor identity is self-reported and unauthenticated"
        );
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}

fn absolute_path(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}
