use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::*;
use crate::store::{
    AcceptanceDecisionKind, AcceptanceStoreDecision, commit_acceptance_transaction_with_fault,
    load_acceptance_decision,
};
use crate::support::compute_sha256;

fn workspace(root: &Path) -> PublicationWorkspace {
    PublicationWorkspace {
        project_root: root.to_path_buf(),
        wiki_content_dir: root.join("wiki_content"),
        state_dir: root.join(".wikitool"),
        acceptance_store_path: root
            .join(".wikitool")
            .join("acceptance")
            .join("acceptance.sqlite3"),
    }
}

fn authority(target: &str, adapter: &str, policy_seed: &str) -> ArticlePublicationAuthority {
    ArticlePublicationAuthority {
        target_api_url: target.to_string(),
        site_adapter_id: adapter.to_string(),
        publication_policy_sha256: compute_sha256(policy_seed),
    }
}

fn lint(content: &str) -> ArticleAcceptanceLintSummary {
    ArticleAcceptanceLintSummary {
        content_sha256: compute_sha256(content),
        errors: 0,
        warnings: 0,
        suggestions: 0,
        warnings_explicitly_accepted: false,
    }
}

fn write_draft(paths: &PublicationWorkspace, name: &str, content: &str) -> PathBuf {
    let path = paths.state_dir.join("drafts").join(name);
    fs::create_dir_all(path.parent().expect("draft parent")).expect("create drafts");
    fs::write(&path, content).expect("write draft");
    path
}

#[derive(Debug)]
struct TestEvidence;

impl ArticleReviewEvidenceProvider for TestEvidence {
    fn target_relative_path(&self, title: &str) -> Result<String> {
        Ok(format!(
            "wiki_content/Main/{}.wiki",
            title
                .replace(' ', "_")
                .replace('/', "___")
                .replace(':', "--")
        ))
    }

    fn lint(&self, source_path: &Path, _title: &str) -> Result<ArticleReviewLintSnapshot> {
        let content = fs::read_to_string(source_path)?;
        Ok(ArticleReviewLintSnapshot {
            site_adapter_id: "test-policy".to_string(),
            content_sha256: compute_sha256(&content),
            errors: usize::from(content.contains("LINT_ERROR")),
            warnings: usize::from(content.contains("LINT_WARNING")),
            suggestions: 0,
            issues: Vec::new(),
        })
    }
}

#[test]
fn acceptance_is_bound_to_exact_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let content = "Specific encyclopedic prose.\n";
    let draft = write_draft(&paths, "Exact.wiki", content);
    let target = "wiki_content/Main/Exact.wiki";

    record_article_acceptance(
        &paths,
        &authority,
        ArticleAcceptanceRequest {
            article_path: &draft,
            title: "Exact",
            target_relative_path: target,
            human_editor_claim: "named-human",
            prose_origin: ArticleProseOrigin::AgentDraft,
            lint: lint(content),
        },
    )
    .expect("record acceptance");
    verify_article_acceptance(&paths, &authority, &draft, "Exact", target)
        .expect("verify exact bytes");

    fs::write(&draft, "Changed after acceptance.\n").expect("mutate draft");
    let error = verify_article_acceptance(&paths, &authority, &draft, "Exact", target)
        .expect_err("changed bytes must fail");
    assert!(error.to_string().contains("changed after"));
}

#[test]
fn acceptance_fails_closed_after_target_or_adapter_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let accepted_authority = authority("https://one.test/api.php", "site-a", "policy-a");
    let content = "Authority-bound prose.\n";
    let draft = write_draft(&paths, "Authority.wiki", content);
    let target = "wiki_content/Main/Authority.wiki";
    record_article_acceptance(
        &paths,
        &accepted_authority,
        ArticleAcceptanceRequest {
            article_path: &draft,
            title: "Authority",
            target_relative_path: target,
            human_editor_claim: "named-human",
            prose_origin: ArticleProseOrigin::HumanRevision,
            lint: lint(content),
        },
    )
    .expect("accept");

    let target_change = authority("https://two.test/api.php", "site-a", "policy-a");
    assert!(
        verify_article_acceptance(&paths, &target_change, &draft, "Authority", target)
            .expect_err("target change must fail")
            .to_string()
            .contains("belongs to wiki target")
    );
    let adapter_change = authority("https://one.test/api.php", "site-b", "policy-b");
    assert!(
        verify_article_acceptance(&paths, &adapter_change, &draft, "Authority", target)
            .expect_err("adapter change must fail")
            .to_string()
            .contains("publication policy changed")
    );
}

#[test]
fn reacceptance_retains_historical_decision_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let draft = write_draft(&paths, "History.wiki", "First.\n");
    let target = "wiki_content/Main/History.wiki";
    record_article_acceptance(
        &paths,
        &authority,
        ArticleAcceptanceRequest {
            article_path: &draft,
            title: "History",
            target_relative_path: target,
            human_editor_claim: "named-human",
            prose_origin: ArticleProseOrigin::HumanRevision,
            lint: lint("First.\n"),
        },
    )
    .expect("first acceptance");
    let first =
        load_accepted_article(&paths, &authority, &draft, "History", target).expect("load first");

    fs::write(&draft, "Second.\n").expect("second draft");
    record_article_acceptance(
        &paths,
        &authority,
        ArticleAcceptanceRequest {
            article_path: &draft,
            title: "History",
            target_relative_path: target,
            human_editor_claim: "named-human",
            prose_origin: ArticleProseOrigin::HumanRevision,
            lint: lint("Second.\n"),
        },
    )
    .expect("second acceptance");
    let second =
        load_accepted_article(&paths, &authority, &draft, "History", target).expect("load second");
    assert_ne!(first.decision_id, second.decision_id);
    load_acceptance_decision(&paths, &first.decision_id).expect("historical decision retained");
}

#[test]
fn acceptance_store_is_independent_of_disposable_catalog_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let content = "Durable prose.\n";
    let draft = write_draft(&paths, "Durable.wiki", content);
    let target = "wiki_content/Main/Durable.wiki";
    record_article_acceptance(
        &paths,
        &authority,
        ArticleAcceptanceRequest {
            article_path: &draft,
            title: "Durable",
            target_relative_path: target,
            human_editor_claim: "named-human",
            prose_origin: ArticleProseOrigin::HumanRevision,
            lint: lint(content),
        },
    )
    .expect("accept");
    let catalog = paths.state_dir.join("data/wikitool.db");
    fs::create_dir_all(catalog.parent().expect("catalog parent")).expect("create catalog parent");
    fs::write(&catalog, b"disposable").expect("write catalog");
    fs::remove_file(catalog).expect("reset catalog");
    verify_article_acceptance(&paths, &authority, &draft, "Durable", target)
        .expect("durable decision survives");
}

fn changeset_inputs(paths: &PublicationWorkspace) -> Vec<ArticleReviewChangesetInput> {
    vec![
        ArticleReviewChangesetInput {
            source_path: write_draft(paths, "Alpha.wiki", "Alpha prose.\n"),
            title: "Alpha".to_string(),
            prose_origin: ArticleProseOrigin::AgentDraft,
        },
        ArticleReviewChangesetInput {
            source_path: write_draft(paths, "Beta.wiki", "Beta prose.\n"),
            title: "Beta".to_string(),
            prose_origin: ArticleProseOrigin::CollaborativeDraft,
        },
    ]
}

#[test]
fn one_changeset_decision_commits_every_exact_content_authorization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let inputs = changeset_inputs(&paths);
    let manifest = paths.state_dir.join("review/batch.json");
    prepare_article_review_changeset(
        &paths,
        &authority,
        &TestEvidence,
        &manifest,
        inputs.clone(),
        false,
    )
    .expect("prepare");
    let result = accept_article_review_changeset(
        &paths,
        &authority,
        &TestEvidence,
        &manifest,
        "named-human",
        ArticleChangesetWarningPolicy::RequireNone,
    )
    .expect("accept batch");
    assert_eq!(result.decision.items.len(), 2);
    for input in inputs {
        let target = TestEvidence
            .target_relative_path(&input.title)
            .expect("target");
        let accepted = load_accepted_article(
            &paths,
            &authority,
            &input.source_path,
            &input.title,
            &target,
        )
        .expect("accepted item");
        assert_eq!(
            accepted.changeset_sha256.as_deref(),
            Some(result.decision.changeset_sha256.as_str())
        );
    }
}

#[test]
fn changed_item_blocks_entire_changeset_before_any_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let inputs = changeset_inputs(&paths);
    let manifest = paths.state_dir.join("review/changed.json");
    prepare_article_review_changeset(
        &paths,
        &authority,
        &TestEvidence,
        &manifest,
        inputs.clone(),
        false,
    )
    .expect("prepare");
    fs::write(&inputs[1].source_path, "Changed.\n").expect("mutate item");
    let error = accept_article_review_changeset(
        &paths,
        &authority,
        &TestEvidence,
        &manifest,
        "named-human",
        ArticleChangesetWarningPolicy::RequireNone,
    )
    .expect_err("batch must fail");
    assert!(error.to_string().contains("changed after preparation"));
    assert!(!paths.acceptance_store_path.exists());
}

#[test]
fn case_colliding_changeset_identities_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let inputs = vec![
        ArticleReviewChangesetInput {
            source_path: write_draft(&paths, "Foo-a.wiki", "One.\n"),
            title: "Foo".to_string(),
            prose_origin: ArticleProseOrigin::HumanRevision,
        },
        ArticleReviewChangesetInput {
            source_path: write_draft(&paths, "Foo-b.wiki", "Two.\n"),
            title: "foo".to_string(),
            prose_origin: ArticleProseOrigin::HumanRevision,
        },
    ];
    let error = prepare_article_review_changeset(
        &paths,
        &authority,
        &TestEvidence,
        &paths.state_dir.join("review/collision.json"),
        inputs,
        false,
    )
    .expect_err("portable collision must fail");
    assert!(error.to_string().contains("collides"));
    assert!(!paths.acceptance_store_path.exists());
}

#[test]
fn changeset_acceptance_fails_before_writes_after_authority_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let first = authority("https://one.test/api.php", "site-a", "policy-a");
    let changed = authority("https://two.test/api.php", "site-b", "policy-b");
    let manifest = paths.state_dir.join("review/authority.json");
    prepare_article_review_changeset(
        &paths,
        &first,
        &TestEvidence,
        &manifest,
        changeset_inputs(&paths),
        false,
    )
    .expect("prepare");
    let error = accept_article_review_changeset(
        &paths,
        &changed,
        &TestEvidence,
        &manifest,
        "named-human",
        ArticleChangesetWarningPolicy::RequireNone,
    )
    .expect_err("authority drift must fail");
    assert!(error.to_string().contains("belongs to wiki target"));
    assert!(!paths.acceptance_store_path.exists());
}

#[test]
fn injected_batch_failure_rolls_back_decision_and_every_authorization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = workspace(temp.path());
    let authority = authority("https://example.test/api.php", "site", "policy");
    let changeset_sha256 = compute_sha256("changeset");
    let decision_id = compute_sha256("decision");
    let accepted_at_unix = 42;
    let decision = AcceptanceStoreDecision {
        decision_id: decision_id.clone(),
        kind: AcceptanceDecisionKind::ArticleChangeset,
        changeset_sha256: Some(changeset_sha256.clone()),
        human_editor_claim: "named-human".to_string(),
        editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE.to_string(),
        decision: ACCEPTANCE_DECISION.to_string(),
        accepted_at_unix,
        publication_authority: authority.clone(),
        receipt_json: "{}".to_string(),
    };
    let ledgers = ["Alpha", "Beta"].map(|title| ArticleAcceptanceLedgerEntry {
        schema_version: ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION.to_string(),
        title: title.to_string(),
        source_relative_path: format!(".wikitool/drafts/{title}.wiki"),
        target_relative_path: format!("wiki_content/Main/{title}.wiki"),
        content_sha256: compute_sha256(title),
        human_editor_claim: "named-human".to_string(),
        editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE.to_string(),
        prose_origin: ArticleProseOrigin::HumanRevision,
        decision: ACCEPTANCE_DECISION.to_string(),
        accepted_at_unix,
        lint_errors: 0,
        lint_warnings: 0,
        lint_suggestions: 0,
        warnings_explicitly_accepted: false,
        warning_decision: Some(ArticleWarningDecision::NoWarnings),
        changeset_decision: Some(ArticleAcceptanceDecisionBinding {
            decision_id: decision_id.clone(),
            changeset_sha256: changeset_sha256.clone(),
        }),
        publication_authority: Some(authority.clone()),
    });

    let error = commit_acceptance_transaction_with_fault(&paths, &decision, &ledgers, 1)
        .expect_err("injected write failure");
    assert!(error.to_string().contains("injected"));
    assert!(load_acceptance_decision(&paths, &decision_id).is_err());
    assert!(
        crate::store::load_article_authorization(&paths, "wiki_content/Main/Alpha.wiki").is_err()
    );
}
