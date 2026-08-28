use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

use super::storage::{SyncStateFaultPoint, inject_sync_state_fault_once};
use super::{
    DiffBaselineStatus, DiffChangeType, DiffOptions, EditConstraint, NS_CATEGORY, NS_MAIN,
    NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE, PageTimestampInfo, PullCoverage, PullOptions,
    PushOptions, RemoteMutationEffectKind, RemotePage, RevisionLineageEntry, SyncPlanOptions,
    SyncSelection, SyncStateError, WikiReadApi, WikiWriteApi,
};
use crate::support::compute_sha256;
use crate::{
    DeleteLogEntry, DeleteOutcome, DeleteReceipt, EditReceipt, PreparedPublication,
    PublicationCandidate, PublicationPreflight, PublicationProvenance, RemoteDeleteError,
    SyncPathEffectKind, SyncProjectPaths, SyncTargetIdentity,
};

#[derive(Default)]
struct MockApi {
    all_pages_by_namespace: BTreeMap<i32, Vec<String>>,
    recent_changes: Vec<String>,
    category_members: Vec<String>,
    page_contents: BTreeMap<String, RemotePage>,
    revisions_by_id: BTreeMap<i64, RemotePage>,
    revision_lineage: BTreeMap<String, Vec<RevisionLineageEntry>>,
    exact_revision_override: Option<RemotePage>,
    exact_revision_error: Option<String>,
    current_after_exact_revision: Option<RemotePage>,
    exact_revision_requests: Vec<i64>,
    edit_error_after_apply: Option<String>,
    page_timestamps: BTreeMap<String, PageTimestampInfo>,
    timestamp_responses: Vec<Vec<PageTimestampInfo>>,
    timestamp_batches: Vec<Vec<String>>,
    edited_pages: Vec<String>,
    edit_summaries: Vec<(String, String)>,
    edit_constraints: Vec<(String, EditConstraint)>,
    deleted_pages: Vec<String>,
    delete_reasons: Vec<(String, String)>,
    delete_receipt_title: Option<String>,
    delete_error_after_apply: Option<String>,
    delete_log_entries: Vec<DeleteLogEntry>,
    delete_hook: Option<Box<dyn FnMut()>>,
    delete_postread_page: Option<RemotePage>,
    login_required: bool,
    logged_in: bool,
    request_count: usize,
    target_api_url: String,
}

impl WikiReadApi for MockApi {
    fn target_api_url(&self) -> &str {
        if self.target_api_url.is_empty() {
            "https://wiki-a.example/api.php"
        } else {
            &self.target_api_url
        }
    }

    fn get_all_pages(&mut self, namespace: i32) -> anyhow::Result<Vec<String>> {
        self.request_count += 1;
        Ok(self
            .all_pages_by_namespace
            .get(&namespace)
            .cloned()
            .unwrap_or_default())
    }

    fn get_category_members(&mut self, _category: &str) -> anyhow::Result<Vec<String>> {
        self.request_count += 1;
        Ok(self.category_members.clone())
    }

    fn get_recent_changes(
        &mut self,
        _since: &str,
        _namespaces: &[i32],
    ) -> anyhow::Result<Vec<String>> {
        self.request_count += 1;
        Ok(self.recent_changes.clone())
    }

    fn get_page_contents(&mut self, titles: &[String]) -> anyhow::Result<Vec<RemotePage>> {
        self.request_count += 1;
        let mut output = Vec::new();
        for title in titles {
            if let Some(page) = self.page_contents.get(title) {
                output.push(page.clone());
            }
        }
        Ok(output)
    }

    fn get_revision_by_id(&mut self, revision_id: i64) -> anyhow::Result<Option<RemotePage>> {
        self.request_count += 1;
        self.exact_revision_requests.push(revision_id);
        if let Some(message) = &self.exact_revision_error {
            anyhow::bail!(message.clone());
        }
        let result = if let Some(page) = &self.exact_revision_override {
            Some(page.clone())
        } else {
            self.revisions_by_id.get(&revision_id).cloned()
        };
        if let Some(current) = self.current_after_exact_revision.take() {
            self.page_contents
                .insert(current.title.clone(), current.clone());
            self.page_timestamps.insert(
                current.title.clone(),
                PageTimestampInfo {
                    title: current.title.clone(),
                    timestamp: current.timestamp.clone(),
                    revision_id: current.revision_id,
                },
            );
        }
        Ok(result)
    }

    fn get_revision_lineage(&mut self, title: &str) -> anyhow::Result<Vec<RevisionLineageEntry>> {
        self.request_count += 1;
        Ok(self
            .revision_lineage
            .get(title)
            .cloned()
            .unwrap_or_default())
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}

impl WikiWriteApi for MockApi {
    fn login(&mut self, _username: &str, _password: &str) -> anyhow::Result<()> {
        self.request_count += 1;
        self.logged_in = true;
        Ok(())
    }

    fn get_page_timestamps(&mut self, titles: &[String]) -> anyhow::Result<Vec<PageTimestampInfo>> {
        self.request_count += 1;
        self.timestamp_batches.push(titles.to_vec());
        if !self.timestamp_responses.is_empty() {
            return Ok(self.timestamp_responses.remove(0));
        }
        let mut output = Vec::new();
        for title in titles {
            if let Some(item) = self.page_timestamps.get(title) {
                output.push(item.clone());
            }
        }
        Ok(output)
    }

    fn edit_page(
        &mut self,
        title: &str,
        content: &str,
        summary: &str,
        constraint: EditConstraint,
    ) -> anyhow::Result<EditReceipt> {
        self.request_count += 1;
        if self.login_required && !self.logged_in {
            anyhow::bail!("not logged in");
        }
        self.edited_pages.push(title.to_string());
        self.edit_summaries
            .push((title.to_string(), summary.to_string()));
        self.edit_constraints.push((title.to_string(), constraint));
        let old_revision_id = match constraint {
            EditConstraint::CreateOnly => 0,
            EditConstraint::ExistingRevision { revision_id } => revision_id,
        };
        let new_revision_id = 9_000 + i64::try_from(self.edited_pages.len()).expect("edit count");
        let page = RemotePage {
            title: title.to_string(),
            namespace: NS_MAIN,
            page_id: 9000,
            revision_id: new_revision_id,
            timestamp: "2026-02-20T00:00:00Z".to_string(),
            content: content.to_string(),
        };
        self.page_contents.insert(title.to_string(), page.clone());
        self.revisions_by_id.insert(new_revision_id, page.clone());
        self.revision_lineage
            .entry(title.to_string())
            .or_default()
            .insert(
                0,
                RevisionLineageEntry {
                    title: title.to_string(),
                    page_id: page.page_id,
                    revision_id: new_revision_id,
                    timestamp: page.timestamp.clone(),
                    comment: Some(summary.to_string()),
                    comment_hidden: false,
                    content: Some(content.to_string()),
                },
            );
        self.page_timestamps.insert(
            title.to_string(),
            PageTimestampInfo {
                title: title.to_string(),
                timestamp: page.timestamp.clone(),
                revision_id: page.revision_id,
            },
        );
        if let Some(message) = &self.edit_error_after_apply {
            anyhow::bail!(message.clone());
        }
        Ok(EditReceipt {
            title: title.to_string(),
            page_id: page.page_id,
            old_revision_id,
            new_revision_id,
            new_timestamp: page.timestamp,
        })
    }

    fn delete_page(&mut self, title: &str, reason: &str) -> anyhow::Result<DeleteOutcome> {
        self.request_count += 1;
        if self.login_required && !self.logged_in {
            anyhow::bail!("not logged in");
        }
        self.deleted_pages.push(title.to_string());
        self.delete_reasons
            .push((title.to_string(), reason.to_string()));
        self.page_timestamps.remove(title);
        self.page_contents.remove(title);
        let log_id = 7_001;
        self.delete_log_entries.push(DeleteLogEntry {
            log_id,
            title: title.to_string(),
            timestamp: "2026-02-20T00:00:01Z".to_string(),
            comment: Some(reason.to_string()),
            comment_hidden: false,
            user: Some("bot".to_string()),
        });
        if let Some(hook) = &mut self.delete_hook {
            hook();
        }
        if let Some(page) = &self.delete_postread_page {
            self.page_contents.insert(page.title.clone(), page.clone());
            self.page_timestamps.insert(
                page.title.clone(),
                PageTimestampInfo {
                    title: page.title.clone(),
                    timestamp: page.timestamp.clone(),
                    revision_id: page.revision_id,
                },
            );
        }
        if let Some(message) = &self.delete_error_after_apply {
            anyhow::bail!(message.clone());
        }
        Ok(DeleteOutcome::Deleted(DeleteReceipt {
            title: self
                .delete_receipt_title
                .clone()
                .unwrap_or_else(|| title.to_string()),
            log_id,
        }))
    }

    fn get_delete_log_entries(&mut self, title: &str) -> anyhow::Result<Vec<DeleteLogEntry>> {
        self.request_count += 1;
        Ok(self
            .delete_log_entries
            .iter()
            .filter(|entry| {
                super::mediawiki_title_identity(&entry.title)
                    == super::mediawiki_title_identity(title)
            })
            .cloned()
            .collect())
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write file");
}

struct PassThroughPublicationPreflight;

impl PublicationPreflight for PassThroughPublicationPreflight {
    fn prepare(&self, candidate: PublicationCandidate<'_>) -> anyhow::Result<PreparedPublication> {
        Ok(PreparedPublication {
            content: fs::read_to_string(candidate.absolute_path)?,
            provenance: None,
        })
    }
}

struct ProvenancePublicationPreflight;

impl PublicationPreflight for ProvenancePublicationPreflight {
    fn prepare(&self, candidate: PublicationCandidate<'_>) -> anyhow::Result<PreparedPublication> {
        let content = fs::read_to_string(candidate.absolute_path)?;
        Ok(PreparedPublication {
            provenance: Some(PublicationProvenance {
                content_sha256: compute_sha256(&content),
                accepted_at_unix: 1_777_000_000,
                prose_origin: "human_revision".to_string(),
                editor_identity_assurance: "self_reported_unverified".to_string(),
                warning_decision: "no_warnings".to_string(),
                decision_id: compute_sha256("test publication decision"),
                changeset_sha256: None,
                publication_authority: None,
            }),
            content,
        })
    }
}

struct RejectingPublicationPreflight;

impl PublicationPreflight for RejectingPublicationPreflight {
    fn prepare(&self, candidate: PublicationCandidate<'_>) -> anyhow::Result<PreparedPublication> {
        anyhow::bail!("{} lacks publication authorization", candidate.title)
    }
}

fn accept_main_article(_paths: &SyncProjectPaths, _title: &str) {
    // Publication authority is independently tested by wikitool_publication.
}

fn paths(project_root: &Path) -> SyncProjectPaths {
    let paths = SyncProjectPaths {
        project_root: project_root.to_path_buf(),
        wiki_content_dir: project_root.join("wiki_content"),
        templates_dir: project_root.join("templates"),
        sync_store_path: project_root.join(".wikitool/sync/sync.sqlite3"),
        custom_namespaces: Vec::new(),
        template_category_mappings: Vec::new(),
    };
    fs::create_dir_all(test_state_dir(&paths)).expect("create test state");
    paths
}

fn test_state_dir(paths: &SyncProjectPaths) -> std::path::PathBuf {
    paths.project_root.join(".wikitool")
}

fn target(api_url: &str) -> SyncTargetIdentity {
    SyncTargetIdentity::from_api_url(api_url).expect("target identity")
}

fn default_target() -> SyncTargetIdentity {
    target("https://wiki-a.example/api.php")
}

fn pull_from_remote_with_api<A: WikiReadApi>(
    paths: &SyncProjectPaths,
    options: &PullOptions,
    api: &mut A,
) -> anyhow::Result<super::PullReport> {
    let target = target(api.target_api_url());
    super::pull_from_remote_with_api(paths, options, &target, api)
}

fn plan_sync_changes(
    paths: &SyncProjectPaths,
    options: &SyncPlanOptions,
) -> anyhow::Result<Option<super::SyncPlanReport>> {
    super::plan_sync_changes(paths, options, &default_target())
}

fn diff_local_against_sync(
    paths: &SyncProjectPaths,
    options: &DiffOptions,
) -> anyhow::Result<Option<super::DiffReport>> {
    super::diff_local_against_sync(paths, options, &default_target())
}

fn collect_changed_article_paths(
    paths: &SyncProjectPaths,
    selection: &SyncSelection,
    include_selected_redirects: bool,
) -> anyhow::Result<Option<Vec<String>>> {
    super::collect_changed_article_paths(
        paths,
        selection,
        include_selected_redirects,
        &default_target(),
    )
}

fn push_to_remote_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    options: &PushOptions,
    api: &mut A,
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<super::PushReport> {
    let target = target(api.target_api_url());
    if options.dry_run {
        return super::push_to_remote_with_api_and_preflight(
            paths,
            options,
            &target,
            api,
            credentials,
            &ProvenancePublicationPreflight,
        );
    }
    let mut preview_options = options.clone();
    preview_options.dry_run = true;
    preview_options.apply_plan_id = None;
    let preview = super::push_to_remote_with_api_and_preflight(
        paths,
        &preview_options,
        &target,
        api,
        None,
        &ProvenancePublicationPreflight,
    )?;
    let mut apply_options = options.clone();
    apply_options.apply_plan_id = preview.plan_id;
    super::push_to_remote_with_api_and_preflight(
        paths,
        &apply_options,
        &target,
        api,
        credentials,
        &ProvenancePublicationPreflight,
    )
}

fn delete_remote_page_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    title: &str,
    reason: &str,
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<super::RemoteDeleteReport> {
    let local_path = paths
        .wiki_content_dir
        .join("Main")
        .join(format!("{title}.wiki"));
    let local_content_sha256 = local_path
        .is_file()
        .then(|| fs::read_to_string(&local_path).map(|content| compute_sha256(&content)))
        .transpose()?;
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: false,
        backup_directory: None,
        backup_path: None,
        local_content_sha256,
    };
    if credentials.is_none() {
        return super::apply_remote_delete_with_api(
            paths,
            target,
            api,
            super::RemoteDeleteApplyRequest {
                title,
                reason,
                local_effect_policy: policy,
                plan_id: Some("not-used"),
                credentials: None,
            },
        );
    }
    let plan =
        super::plan_remote_delete_with_api(paths, target, api, title, reason, policy.clone())?;
    super::apply_remote_delete_with_api(
        paths,
        target,
        api,
        super::RemoteDeleteApplyRequest {
            title,
            reason,
            local_effect_policy: policy,
            plan_id: Some(&plan.plan_id),
            credentials,
        },
    )
}

fn authoritative_namespaces() -> Vec<i32> {
    vec![NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI]
}

fn assert_global_planning_is_unestablished(paths: &SyncProjectPaths) {
    let error = plan_sync_changes(
        paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect_err("global planning must remain locked");
    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::Unestablished { .. })
    ));
}

fn base_page(title: &str, content: &str) -> RemotePage {
    RemotePage {
        title: title.to_string(),
        namespace: NS_MAIN,
        page_id: 100,
        revision_id: 200,
        timestamp: "2026-02-19T00:00:00Z".to_string(),
        content: content.to_string(),
    }
}

type StoredSyncPagePair = (Option<(String, String)>, Option<(String, String)>);

fn stored_sync_page_pair(paths: &SyncProjectPaths, title: &str) -> StoredSyncPagePair {
    let connection = super::open_sync_connection(paths).expect("open sync store");
    let key = super::normalized_title_key(title);
    let ledger = super::load_sync_ledger_map(&connection, true)
        .expect("load sync ledger")
        .remove(&key)
        .map(|entry| (entry.relative_path, entry.content_hash));
    let snapshot = super::load_sync_snapshot_map(&connection)
        .expect("load sync snapshots")
        .remove(&key)
        .map(|entry| (entry.relative_path, entry.content_text));
    (ledger, snapshot)
}

#[derive(Debug, PartialEq, Eq)]
struct StoredEditMutation {
    phase: String,
    intended_content_sha256: String,
    intended_normalized_sha256: String,
    response_old_revision_id: Option<i64>,
    response_new_revision_id: Option<i64>,
    response_new_timestamp: Option<String>,
    detail: Option<String>,
}

fn stored_edit_mutation(paths: &SyncProjectPaths, title: &str) -> StoredEditMutation {
    let connection = super::open_sync_connection(paths).expect("open sync store");
    connection
        .query_row(
            "SELECT phase, intended_content_sha256, intended_normalized_sha256,
                    response_old_revision_id, response_new_revision_id,
                    response_new_timestamp, detail
             FROM sync_edit_mutations
             WHERE title = ?1
             ORDER BY mutation_id DESC
             LIMIT 1",
            [title],
            |row| {
                Ok(StoredEditMutation {
                    phase: row.get(0)?,
                    intended_content_sha256: row.get(1)?,
                    intended_normalized_sha256: row.get(2)?,
                    response_old_revision_id: row.get(3)?,
                    response_new_revision_id: row.get(4)?,
                    response_new_timestamp: row.get(5)?,
                    detail: row.get(6)?,
                })
            },
        )
        .expect("read durable edit mutation")
}

fn modified_alpha_fixture() -> (tempfile::TempDir, SyncProjectPaths, MockApi) {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let remote = base_page("Alpha", "alpha body");
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), remote.clone());
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit\n",
    );
    (temp, paths, api)
}

fn write_push_options(summary: &str) -> PushOptions {
    PushOptions {
        summary: summary.to_string(),
        dry_run: false,
        force: false,
        delete: false,
        include_templates: false,
        categories_only: false,
        all: true,
        selection: SyncSelection::default(),
        apply_plan_id: None,
    }
}

#[test]
fn sync_title_keys_preserve_case_after_initial_character() {
    assert_eq!(
        super::normalized_title_key("network_Spirituality"),
        super::normalized_title_key("Network Spirituality")
    );
    assert_ne!(
        super::normalized_title_key("Network Spirituality"),
        super::normalized_title_key("Network spirituality")
    );
    assert_eq!(
        super::normalized_title_key("template:infobox Concept"),
        super::normalized_title_key("Template:Infobox Concept")
    );
    assert_ne!(
        super::normalized_title_key("Template:Infobox Concept"),
        super::normalized_title_key("Template:Infobox concept")
    );
}

#[test]
fn sync_planning_fails_typed_when_revision_state_is_missing() {
    let temp = tempdir().expect("tempdir");
    let paths = paths(temp.path());

    let error = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect_err("planning must not reinterpret missing state as zero changes");
    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::Missing { path }) if path == &paths.sync_store_path()
    ));
}

#[test]
fn pull_writes_files_and_returns_changed_path_effects() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    api.page_contents
        .insert("Beta".to_string(), base_page("Beta", "[[Alpha]]"));

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull");

    assert!(report.success);
    assert_eq!(report.created, 2);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 0);
    assert!(
        paths
            .wiki_content_dir
            .join("Main")
            .join("Alpha.wiki")
            .exists()
    );
    assert!(
        paths
            .wiki_content_dir
            .join("Main")
            .join("Beta.wiki")
            .exists()
    );
    assert_eq!(report.changed_paths.len(), 2);
    assert!(report.changed_paths.iter().all(|effect| {
        effect.kind == SyncPathEffectKind::Created
            && effect.relative_path.starts_with("wiki_content/Main/")
    }));
}

#[test]
fn pull_rolls_back_ledger_when_snapshot_persistence_fails() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    let before = stored_sync_page_pair(&paths, "Alpha");

    let mut updated = base_page("Alpha", "alpha remote update");
    updated.revision_id = 201;
    updated.timestamp = "2026-02-20T00:00:00Z".to_string();
    api.page_contents.insert("Alpha".to_string(), updated);
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterLedgerUpsert);

    let error = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect_err("injected snapshot failure must fail the pull");

    assert!(format!("{error:#}").contains("injected sync-state transaction failure"));
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        fs::read_to_string(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
            .expect("read locally refreshed page"),
        "alpha remote update"
    );
}

#[test]
fn full_main_only_pull_does_not_establish_global_authority() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
            coverage: PullCoverage::Scoped,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("scoped pull");

    assert!(report.success);
    assert!(!report.global_baseline_established);
    assert_global_planning_is_unestablished(&paths);
}

#[test]
fn category_pull_does_not_establish_global_authority() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi {
        category_members: vec!["Alpha".to_string()],
        ..Default::default()
    };
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: Some("Category:Selected".to_string()),
            full: true,
            coverage: PullCoverage::Scoped,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("category pull");

    assert!(report.success);
    assert!(!report.global_baseline_established);
    assert_global_planning_is_unestablished(&paths);
}

#[test]
fn pull_skips_modified_local_when_overwrite_is_disabled() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "local edited",
    );

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "remote version"));

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull");

    assert_eq!(report.created, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 1);
    assert!(!report.success);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("complete global sync baseline"))
    );
    let current = fs::read_to_string(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
        .expect("read local file");
    assert_eq!(current, "local edited");
    let error = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect_err("a skipped first pull must not establish a baseline");
    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::Unestablished { .. })
    ));
}

#[test]
fn ambiguous_legacy_row_cannot_satisfy_a_current_global_baseline() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "local edited",
    );

    let connection = super::open_sync_connection(&paths).expect("create unestablished store");
    connection
        .execute(
            "INSERT INTO sync_ledger_pages (
                title, namespace, relative_path, content_hash, wiki_modified_at,
                revision_id, page_id, is_redirect, redirect_target, last_synced_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL, ?8)",
            rusqlite::params![
                "Alpha",
                NS_MAIN,
                "wiki_content/Main/Alpha.wiki",
                "legacy-unknown-target-hash",
                "2025-01-01T00:00:00Z",
                200,
                100,
                1_i64,
            ],
        )
        .expect("insert ambiguous legacy row");
    drop(connection);

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "remote version"));

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("authoritative pull report");

    assert!(!report.success);
    assert!(!report.global_baseline_established);
    assert_global_planning_is_unestablished(&paths);
}

#[test]
fn successful_empty_pull_establishes_an_empty_baseline() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("empty pull");
    assert!(report.success);
    assert!(report.global_baseline_established);

    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan against empty baseline")
    .expect("sync plan");
    assert!(plan.changes.is_empty());
}

#[test]
fn authoritative_pull_removes_unchanged_local_page_deleted_remotely() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.all_pages_by_namespace.insert(NS_MAIN, Vec::new());
    api.page_contents.clear();

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("authoritative deletion pull");

    assert!(report.success);
    assert_eq!(report.deleted, 1);
    assert!(
        report
            .pages
            .iter()
            .any(|page| { page.title == "Alpha" && page.action == "deleted_remote_absent" })
    );
    assert!(!paths.wiki_content_dir.join("Main/Alpha.wiki").exists());
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), (None, None));
    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan after remote deletion")
    .expect("established baseline");
    assert!(plan.changes.is_empty());
}

#[test]
fn authoritative_pull_retains_modified_remote_delete_as_conflict_identity() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    let before = stored_sync_page_pair(&paths, "Alpha");
    write_file(
        &paths.wiki_content_dir.join("Main/Alpha.wiki"),
        "alpha local edit after remote deletion",
    );
    accept_main_article(&paths, "Alpha");
    api.all_pages_by_namespace.insert(NS_MAIN, Vec::new());
    api.page_contents.clear();

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("authoritative deletion pull");

    assert!(report.success);
    assert_eq!(report.deleted, 0);
    assert!(
        report.pages.iter().any(|page| {
            page.title == "Alpha" && page.action == "retained_remote_delete_conflict"
        })
    );
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan retained conflict")
    .expect("established baseline");
    assert!(plan.changes.iter().any(|change| {
        change.title == "Alpha" && change.change_type == DiffChangeType::ModifiedLocal
    }));

    let push = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test no silent resurrection".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("push dry run");
    assert_eq!(push.conflicts, vec!["Alpha".to_string()]);
    assert!(api.edited_pages.is_empty());
}

#[test]
fn authoritative_remote_delete_failure_keeps_transactional_conflict_identity() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.all_pages_by_namespace.insert(NS_MAIN, Vec::new());
    api.page_contents.clear();
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterLedgerDelete);

    let error = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect_err("injected state cleanup failure must fail the pull");

    assert!(format!("{error:#}").contains("injected sync-state transaction failure"));
    assert!(!paths.wiki_content_dir.join("Main/Alpha.wiki").exists());
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("safe plan after interrupted reconciliation")
    .expect("prior established baseline");
    assert!(plan.changes.iter().any(|change| {
        change.title == "Alpha" && change.change_type == DiffChangeType::DeletedLocal
    }));
}

#[test]
fn authoritative_pull_does_not_touch_adjacent_authority_stores() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let adjacent_store = test_state_dir(&paths).join("acceptance/acceptance.sqlite3");
    write_file(&adjacent_store, "authority sentinel\n");
    let mut api = MockApi::default();

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("authoritative pull");

    assert_eq!(
        fs::read_to_string(adjacent_store).expect("read adjacent authority"),
        "authority sentinel\n"
    );
}

#[test]
fn failed_first_pull_does_not_bind_target_and_allows_corrected_retarget() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let mut api = MockApi {
        target_api_url: "https://typo.example/api.php".to_string(),
        ..MockApi::default()
    };
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Missing response".to_string()]);

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull report");
    assert!(!report.success);
    assert!(!report.global_baseline_established);

    let error = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect_err("failed pull must not create a usable baseline");
    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::Unestablished { .. })
    ));

    api.target_api_url = "https://corrected.example/api.php".to_string();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Recovered".to_string()]);
    api.page_contents.insert(
        "Recovered".to_string(),
        base_page("Recovered", "recovered body"),
    );
    let recovered = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("corrected target pull");
    assert!(recovered.success);
    assert!(recovered.global_baseline_established);
}

#[test]
fn incremental_pull_does_not_advance_checkpoint_when_page_is_skipped() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "local edited",
    );

    let connection = super::open_sync_connection(&paths).expect("open sync db");
    super::initialize_sync_schema(&connection).expect("initialize sync schema");
    super::set_sync_config(&connection, "last_pull_ns_0", "2026-02-01T00:00:00Z")
        .expect("seed pull cursor");

    let mut api = MockApi {
        recent_changes: vec!["Alpha".to_string()],
        ..Default::default()
    };
    let mut remote = base_page("Alpha", "remote version");
    remote.timestamp = "2026-02-20T00:00:00Z".to_string();
    api.page_contents.insert("Alpha".to_string(), remote);

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: vec![NS_MAIN],
            category: None,
            full: false,
            coverage: PullCoverage::Scoped,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("incremental pull");

    assert_eq!(report.skipped, 1);
    let connection = super::open_sync_connection(&paths).expect("reopen sync db");
    let checkpoint = super::get_sync_config(&connection, "last_pull_ns_0")
        .expect("load pull cursor")
        .expect("pull cursor");
    assert_eq!(checkpoint, "2026-02-01T00:00:00Z");
}

#[test]
fn pull_preserves_old_path_when_redirect_target_has_local_conflict() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    let redirect_path = paths
        .wiki_content_dir
        .join("Main")
        .join("_redirects")
        .join("Alpha.wiki");
    write_file(&redirect_path, "conflicting local redirect");

    let mut redirected = base_page("Alpha", "#REDIRECT [[Beta]]");
    redirected.timestamp = "2026-02-20T00:00:00Z".to_string();
    api.page_contents.insert("Alpha".to_string(), redirected);

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull with redirect conflict");

    assert_eq!(report.skipped, 1);
    assert!(
        paths
            .wiki_content_dir
            .join("Main")
            .join("Alpha.wiki")
            .exists()
    );
    let redirect_content = fs::read_to_string(&redirect_path).expect("read redirect path");
    assert_eq!(redirect_content, "conflicting local redirect");
}

#[test]
fn pull_keeps_case_distinct_redirect_and_target_pages() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut redirect = base_page("Network Spirituality", "#REDIRECT [[Network spirituality]]");
    redirect.page_id = 101;
    redirect.revision_id = 201;
    redirect.timestamp = "2026-02-18T00:00:00Z".to_string();
    let mut target = base_page("Network spirituality", "network spirituality body");
    target.page_id = 102;
    target.revision_id = 202;
    target.timestamp = "2026-02-19T00:00:00Z".to_string();

    let mut api = MockApi::default();
    api.all_pages_by_namespace.insert(
        NS_MAIN,
        vec![
            "Network Spirituality".to_string(),
            "Network spirituality".to_string(),
        ],
    );
    api.page_contents
        .insert("Network Spirituality".to_string(), redirect);
    api.page_contents
        .insert("Network spirituality".to_string(), target);

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull");

    assert!(report.success);
    assert_eq!(report.created, 2);
    let redirect_content = fs::read_to_string(
        paths
            .wiki_content_dir
            .join("Main")
            .join("_redirects")
            .join("Network_Spirituality.wiki"),
    )
    .expect("read redirect");
    assert_eq!(redirect_content, "#REDIRECT [[Network spirituality]]");
    let target_content = fs::read_to_string(
        paths
            .wiki_content_dir
            .join("Main")
            .join("Network_spirituality.wiki"),
    )
    .expect("read target");
    assert_eq!(target_content, "network spirituality body");

    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan")
    .expect("plan report");
    assert_eq!(plan.new_local, 0);
    assert_eq!(plan.modified_local, 0);
    assert_eq!(plan.deleted_local, 0);
}

#[test]
fn pull_uses_case_safe_path_for_case_colliding_titles() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(paths.wiki_content_dir.join("Main")).expect("create main dir");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("I_Long_for_Network_Spirituality.wiki"),
        "stale lower body",
    );

    let mut upper = base_page("I Long For Network Spirituality", "upper body");
    upper.page_id = 301;
    upper.revision_id = 401;
    let mut lower = base_page("I Long for Network Spirituality", "lower body");
    lower.page_id = 302;
    lower.revision_id = 402;

    let mut api = MockApi::default();
    api.all_pages_by_namespace.insert(
        NS_MAIN,
        vec![
            "I Long For Network Spirituality".to_string(),
            "I Long for Network Spirituality".to_string(),
        ],
    );
    api.page_contents
        .insert("I Long For Network Spirituality".to_string(), upper);
    api.page_contents
        .insert("I Long for Network Spirituality".to_string(), lower);

    let report = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: true,
        },
        &mut api,
    )
    .expect("pull");

    assert!(report.success);
    assert_eq!(
        fs::read_to_string(
            paths
                .wiki_content_dir
                .join("Main")
                .join("I_Long_for_Network_Spirituality.wiki")
        )
        .expect("read normal path"),
        "lower body"
    );
    let escaped_path = fs::read_dir(paths.wiki_content_dir.join("Main"))
        .expect("read main dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("__mwtitle_"))
        })
        .expect("case-safe path");
    assert_eq!(
        fs::read_to_string(&escaped_path).expect("read escaped path"),
        "upper body"
    );
    let escaped_relative = escaped_path
        .strip_prefix(&paths.project_root)
        .expect("relative escaped path")
        .to_string_lossy()
        .replace('\\', "/");
    assert_eq!(
        crate::relative_path_to_title(&paths, &escaped_relative).expect("parse escaped title"),
        "I Long For Network Spirituality"
    );

    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan")
    .expect("plan report");
    assert_eq!(plan.new_local, 0);
    assert_eq!(plan.modified_local, 0);
    assert_eq!(plan.deleted_local, 0);
}

#[test]
fn diff_detects_new_modified_and_deleted_local_pages() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    api.page_contents
        .insert("Beta".to_string(), base_page("Beta", "beta body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    fs::remove_file(paths.wiki_content_dir.join("Main").join("Beta.wiki")).expect("delete beta");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "gamma local",
    );
    let diff = diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: false,
            categories_only: false,
            include_content: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("diff")
    .expect("diff report");

    assert_eq!(diff.new_local, 1);
    assert_eq!(diff.modified_local, 1);
    assert_eq!(diff.deleted_local, 1);
    assert!(
        diff.changes
            .iter()
            .any(|item| item.title == "Gamma" && item.change_type == DiffChangeType::NewLocal)
    );
    assert!(
        diff.changes
            .iter()
            .any(|item| item.title == "Alpha" && item.change_type == DiffChangeType::ModifiedLocal)
    );
    assert!(
        diff.changes
            .iter()
            .any(|item| item.title == "Beta" && item.change_type == DiffChangeType::DeletedLocal)
    );
}

#[test]
fn push_dry_run_reports_local_changes_without_writes() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            revision_id: 200,
        },
    );

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    accept_main_article(&paths, "Alpha");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "gamma local",
    );
    accept_main_article(&paths, "Gamma");

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test dry run".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("push dry run");

    assert!(report.dry_run);
    assert_eq!(report.created, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(api.edited_pages.len(), 0);
    assert!(
        report
            .pages
            .iter()
            .any(|item| item.title == "Alpha" && item.action == "would_update")
    );
    assert!(
        report
            .pages
            .iter()
            .any(|item| item.title == "Gamma" && item.action == "would_create")
    );
    let alpha = report
        .pages
        .iter()
        .find(|item| item.title == "Alpha")
        .and_then(|item| item.acceptance.as_ref())
        .expect("accepted prose provenance");
    assert_eq!(alpha.prose_origin, "human_revision");
    assert_eq!(alpha.editor_identity_assurance, "self_reported_unverified");
    assert_eq!(alpha.warning_decision, "no_warnings");
    assert_eq!(alpha.content_sha256, compute_sha256("alpha local edit"));
    let report_json = serde_json::to_string(&report).expect("serialize push report");
    assert!(!report_json.contains("human-editor"));
}

#[test]
fn push_binds_existing_edits_and_new_pages_to_server_constraints() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let remote = base_page("Alpha", "alpha body");
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), remote.clone());
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "gamma local",
    );
    accept_main_article(&paths, "Alpha");
    accept_main_article(&paths, "Gamma");

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test constrained writes".to_string(),
            dry_run: false,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push");

    assert!(report.success);
    assert!(api.edit_constraints.iter().any(|(title, constraint)| {
        title == "Alpha" && *constraint == EditConstraint::ExistingRevision { revision_id: 200 }
    }));
    assert!(
        api.edit_constraints.iter().any(
            |(title, constraint)| title == "Gamma" && *constraint == EditConstraint::CreateOnly
        )
    );
    assert_eq!(api.edit_summaries.len(), 2);
    assert!(api.edit_summaries.iter().all(|(_, summary)| {
        summary.starts_with("[wikitool-edit:") && summary.ends_with("] test constrained writes")
    }));
    assert!(
        api.edit_summaries
            .iter()
            .all(|(_, summary)| !summary.contains("human-editor"))
    );
}

#[test]
fn push_advances_state_only_after_exact_revision_content_is_verified() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("verify exact revision"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push");

    assert!(report.success);
    assert_eq!(report.updated, 1);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert_eq!(api.exact_revision_requests, vec![9_001]);
    assert_eq!(report.mutation_effects.len(), 1);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::StateAdvanced
    );
    assert_eq!(report.mutation_effects[0].old_revision_id, Some(200));
    assert_eq!(report.mutation_effects[0].new_revision_id, Some(9_001));

    let mutation = stored_edit_mutation(&paths, "Alpha");
    assert_eq!(mutation.phase, "state_advanced");
    assert_eq!(
        mutation.intended_content_sha256,
        compute_sha256("alpha local edit\n")
    );
    assert_eq!(
        mutation.intended_normalized_sha256,
        compute_sha256("alpha local edit")
    );
    assert_eq!(mutation.response_old_revision_id, Some(200));
    assert_eq!(mutation.response_new_revision_id, Some(9_001));
    assert_eq!(
        stored_sync_page_pair(&paths, "Alpha")
            .1
            .map(|(_, content)| content),
        Some("alpha local edit\n".to_string())
    );
}

#[test]
fn push_apply_rejects_stale_content_bound_plan_without_writing() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let push_target = default_target();
    let mut options = write_push_options("content-bound plan");
    options.dry_run = true;
    let preview = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        None,
        &ProvenancePublicationPreflight,
    )
    .expect("preview");
    let preview_plan_id = preview.plan_id.expect("preview plan identity");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha changed after preview",
    );
    options.dry_run = false;
    options.apply_plan_id = Some(preview_plan_id.clone());
    let apply = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        Some(("bot", "pass")),
        &ProvenancePublicationPreflight,
    )
    .expect("stale plan report");

    assert!(!apply.success);
    assert_ne!(apply.plan_id.as_deref(), Some(preview_plan_id.as_str()));
    assert!(
        apply
            .errors
            .iter()
            .any(|error| error.contains("plan mismatch"))
    );
    assert!(api.edited_pages.is_empty());
}

#[test]
fn push_apply_rejects_remote_revision_drift_bound_plan_without_writing() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let push_target = default_target();
    let mut options = write_push_options("remote-revision-bound plan");
    options.dry_run = true;
    let preview = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        None,
        &ProvenancePublicationPreflight,
    )
    .expect("preview");
    let preview_plan_id = preview.plan_id.expect("preview plan identity");

    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:01Z".to_string(),
            revision_id: 201,
        },
    );
    options.dry_run = false;
    options.apply_plan_id = Some(preview_plan_id.clone());
    let apply = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        Some(("bot", "pass")),
        &ProvenancePublicationPreflight,
    )
    .expect("remote-drift plan report");

    assert!(!apply.success);
    assert_ne!(apply.plan_id.as_deref(), Some(preview_plan_id.as_str()));
    assert!(
        apply
            .errors
            .iter()
            .any(|error| error.contains("plan mismatch"))
    );
    assert!(api.edited_pages.is_empty());
}

#[test]
fn push_rejects_implicit_global_scope_and_unplanned_live_apply() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let target = default_target();
    let requests_before = api.request_count;
    let mut options = write_push_options("explicit intent required");
    options.dry_run = true;
    options.all = false;
    let scope_error = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &target,
        &mut api,
        None,
        &ProvenancePublicationPreflight,
    )
    .expect_err("implicit global preview must fail");
    assert!(scope_error.to_string().contains("explicit scope"));
    assert_eq!(api.request_count, requests_before);

    options.dry_run = false;
    options.all = true;
    let apply_error = super::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &target,
        &mut api,
        Some(("bot", "pass")),
        &ProvenancePublicationPreflight,
    )
    .expect_err("live push without preview identity must fail");
    assert!(apply_error.to_string().contains("plan ID"));
    assert_eq!(api.request_count, requests_before);
    assert!(api.edited_pages.is_empty());
}

#[test]
fn push_keeps_authority_unchanged_when_exact_revision_content_mismatches() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.exact_revision_override = Some(RemotePage {
        title: "Alpha".to_string(),
        namespace: NS_MAIN,
        page_id: 9_000,
        revision_id: 9_001,
        timestamp: "2026-02-20T00:00:00Z".to_string(),
        content: "server stored different content".to_string(),
    });

    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("reject mismatched content"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert_eq!(report.updated, 0);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::ReconciliationRequired
    );
    let mutation = stored_edit_mutation(&paths, "Alpha");
    assert_eq!(mutation.phase, "reconciliation_required");
    assert!(
        mutation
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("content hash"))
    );
}

#[test]
fn push_keeps_authority_unchanged_when_exact_revision_identity_mismatches() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.exact_revision_override = Some(RemotePage {
        title: "Alpha".to_string(),
        namespace: NS_MAIN,
        page_id: 9_000,
        revision_id: 99_999,
        timestamp: "2026-02-20T00:00:00Z".to_string(),
        content: "alpha local edit\n".to_string(),
    });

    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("reject mismatched revision"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert!(
        stored_edit_mutation(&paths, "Alpha")
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("instead of 9001"))
    );
}

#[test]
fn push_keeps_response_bound_receipt_when_exact_revision_read_fails() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.exact_revision_error = Some("injected exact-revision read failure".to_string());

    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("read failure"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert_eq!(api.exact_revision_requests, vec![9_001]);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    let mutation = stored_edit_mutation(&paths, "Alpha");
    assert_eq!(mutation.phase, "reconciliation_required");
    assert_eq!(mutation.response_old_revision_id, Some(200));
    assert_eq!(mutation.response_new_revision_id, Some(9_001));
    assert!(
        mutation
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("exact-revision read failure"))
    );
}

#[test]
fn push_retains_prior_authority_when_verified_edit_is_immediately_superseded() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.current_after_exact_revision = Some(RemotePage {
        title: "Alpha".to_string(),
        namespace: NS_MAIN,
        page_id: 9_000,
        revision_id: 9_002,
        timestamp: "2026-02-20T00:00:01Z".to_string(),
        content: "intervening remote edit".to_string(),
    });

    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("interleaved remote edit"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::AppliedThenChanged
    );
    assert_eq!(stored_edit_mutation(&paths, "Alpha").phase, "resolved");
}

#[test]
fn ambiguous_applied_disconnect_is_durable_and_never_retried() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    api.edit_error_after_apply = Some("injected disconnect after apply".to_string());

    let first = push_to_remote_with_api(
        &paths,
        &write_push_options("ambiguous write"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("first push report");
    assert!(!first.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert!(api.exact_revision_requests.is_empty());
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        first.mutation_effects[0].kind,
        RemoteMutationEffectKind::OutcomeAmbiguous
    );
    let mutation = stored_edit_mutation(&paths, "Alpha");
    assert_eq!(mutation.phase, "outcome_ambiguous");
    assert_eq!(mutation.response_new_revision_id, None);

    let mut retry_options = write_push_options("must not replay");
    retry_options.force = true;
    let second = push_to_remote_with_api(&paths, &retry_options, &mut api, Some(("bot", "pass")))
        .expect("second push report");
    assert!(!second.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert!(second.pages.iter().any(|page| {
        page.action == "blocked_unresolved_mutation"
            && page
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("outcome_ambiguous"))
    }));
}

#[test]
fn edit_request_start_failure_is_terminal_not_applied_with_receipt() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let connection = super::open_sync_connection(&paths).expect("open sync store");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_edit_request_start
             BEFORE UPDATE OF request_started_at_unix ON sync_edit_mutations
             WHEN NEW.request_started_at_unix IS NOT NULL
             BEGIN SELECT RAISE(ABORT, 'injected edit request-start failure'); END;",
        )
        .expect("install request-start failure trigger");
    drop(connection);

    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("request start failure"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert!(api.edited_pages.is_empty());
    assert_eq!(report.mutation_effects.len(), 1);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::NotApplied
    );
    assert!(
        super::list_remote_mutations(&paths, &default_target(), true)
            .expect("unresolved list")
            .mutations
            .is_empty()
    );
    let receipt = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Edit,
        report.mutation_effects[0].mutation_id,
    )
    .expect("terminal mutation receipt");
    assert_eq!(receipt.phase, "resolved");
    assert_eq!(receipt.terminal_outcome.as_deref(), Some("not_applied"));
}

#[test]
fn delete_request_start_failure_is_terminal_not_applied_with_receipt() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    fs::remove_file(paths.wiki_content_dir.join("Main/Alpha.wiki"))
        .expect("remove local page to plan remote delete");
    let connection = super::open_sync_connection(&paths).expect("open sync store");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_delete_request_start
             BEFORE UPDATE OF request_started_at_unix ON sync_delete_mutations
             WHEN NEW.request_started_at_unix IS NOT NULL
             BEGIN SELECT RAISE(ABORT, 'injected delete request-start failure'); END;",
        )
        .expect("install request-start failure trigger");
    drop(connection);
    let mut options = write_push_options("delete request start failure");
    options.delete = true;

    let report = push_to_remote_with_api(&paths, &options, &mut api, Some(("bot", "pass")))
        .expect("push report");

    assert!(!report.success);
    assert!(api.deleted_pages.is_empty());
    assert_eq!(report.mutation_effects.len(), 1);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::NotApplied
    );
    assert!(
        super::list_remote_mutations(&paths, &default_target(), true)
            .expect("unresolved list")
            .mutations
            .is_empty()
    );
    let receipt = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        report.mutation_effects[0].mutation_id,
    )
    .expect("terminal mutation receipt");
    assert_eq!(receipt.phase, "resolved");
    assert_eq!(receipt.terminal_outcome.as_deref(), Some("not_applied"));
}

#[test]
fn pull_does_not_rewrite_a_title_with_an_unresolved_edit_mutation() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    let local_before =
        fs::read_to_string(paths.wiki_content_dir.join("Main/Alpha.wiki")).expect("local content");
    api.edit_error_after_apply = Some("injected disconnect after apply".to_string());

    let push = push_to_remote_with_api(
        &paths,
        &write_push_options("ambiguous before pull"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("ambiguous push report");
    assert!(!push.success);

    let pull = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: true,
        },
        &mut api,
    )
    .expect("pull skips unresolved title");

    assert!(pull.success);
    assert!(pull.pages.iter().any(|page| {
        page.title == "Alpha"
            && page.action == "retained_unresolved_mutation"
            && page
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("outcome_ambiguous"))
    }));
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        fs::read_to_string(paths.wiki_content_dir.join("Main/Alpha.wiki"))
            .expect("preserved local content"),
        local_before
    );
}

#[test]
fn operator_close_is_inspectable_and_requires_fresh_pull_before_write() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    api.edit_error_after_apply = Some("injected disconnect after apply".to_string());
    let push = push_to_remote_with_api(
        &paths,
        &write_push_options("ambiguous before closure"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("ambiguous push report");
    assert!(!push.success);
    let pending = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending mutation list");
    let mutation_id = pending.mutations[0].mutation_id;
    let original_detail = pending.mutations[0]
        .detail
        .clone()
        .expect("original ambiguous mutation detail");
    let closure_reason = "revision comments are hidden and remote outcome cannot be proved";

    let closure = super::close_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Edit,
        mutation_id,
        "Operator Example",
        closure_reason,
    )
    .expect("operator closure");
    assert_eq!(closure.previous_phase, "outcome_ambiguous");
    assert_eq!(closure.terminal_outcome, "operator_closed_unresolved");
    let shown = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Edit,
        mutation_id,
    )
    .expect("inspect closed mutation");
    assert_eq!(shown.detail.as_deref(), Some(original_detail.as_str()));
    let stored_closure = shown.closure.expect("durable closure receipt");
    assert_eq!(stored_closure.closure_id, closure.closure_id);
    assert_eq!(stored_closure.actor, "Operator Example");
    assert_eq!(stored_closure.reason, closure_reason);
    assert_eq!(stored_closure.previous_phase, "outcome_ambiguous");
    let listed = super::list_remote_mutations(&paths, &default_target(), false)
        .expect("all mutation receipts");
    let listed_closed = listed
        .mutations
        .iter()
        .find(|mutation| mutation.mutation_id == mutation_id)
        .expect("closed mutation in list");
    assert_eq!(
        listed_closed.detail.as_deref(),
        Some(original_detail.as_str())
    );
    assert_eq!(
        listed_closed
            .closure
            .as_ref()
            .map(|receipt| receipt.reason.as_str()),
        Some(closure_reason)
    );

    write_file(
        &paths.wiki_content_dir.join("Main/Alpha.wiki"),
        "alpha second local edit\n",
    );
    api.edit_error_after_apply = None;
    let mut blocked_options = write_push_options("blocked after operator closure");
    blocked_options.force = true;
    let blocked =
        push_to_remote_with_api(&paths, &blocked_options, &mut api, Some(("bot", "pass")))
            .expect("blocked push report");
    assert!(!blocked.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert!(blocked.pages.iter().any(|page| {
        page.detail
            .as_deref()
            .is_some_and(|detail| detail.contains("invalidated by operator closure"))
    }));

    let pull = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: true,
        },
        &mut api,
    )
    .expect("fresh target-bound pull");
    assert!(pull.success);
    write_file(
        &paths.wiki_content_dir.join("Main/Alpha.wiki"),
        "alpha third local edit\n",
    );
    let applied = push_to_remote_with_api(
        &paths,
        &write_push_options("write after refreshed authority"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push after fresh pull");
    assert!(applied.success);
    assert_eq!(api.edited_pages.len(), 2);
}

#[test]
fn operator_close_prepares_and_revalidates_a_bound_delete_backup() {
    let (_temp, paths, _api) = modified_alpha_fixture();
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let content = fs::read_to_string(&source).expect("local content");
    let backup_directory = paths.project_root.join(".wikitool/sync/backups");
    let backup_path = backup_directory.join("Alpha-close.wiki");
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: true,
        backup_directory: Some(backup_directory.to_string_lossy().to_string()),
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        local_content_sha256: Some(compute_sha256(&content)),
    };
    let connection = super::open_sync_connection(&paths).expect("open sync store");
    let intent = super::storage::begin_delete_mutation(
        &paths,
        &connection,
        &default_target(),
        super::storage::NewDeleteMutation {
            title: "Alpha",
            relative_path: Some("wiki_content/Main/Alpha.wiki"),
            expected_revision_id: 200,
            reason: "operator closure backup test",
            local_effect_policy: &policy,
        },
    )
    .expect("durable pending delete intent");
    drop(connection);

    let closure = super::close_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        intent.mutation_id,
        "Operator Example",
        "external audit established that no request was sent",
    )
    .expect("safe delete closure prepares backup");
    assert_eq!(closure.previous_phase, "intent_persisted");
    assert_eq!(
        fs::read_to_string(&backup_path).expect("bound backup"),
        content
    );
    let shown = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        intent.mutation_id,
    )
    .expect("closed delete receipt");
    assert_eq!(shown.local_effect_status.as_deref(), Some("backup_ready"));
    assert!(shown.closure.is_some());
}

#[test]
fn delete_backup_publication_is_atomic_and_never_clobbers_existing_bytes() {
    let temp = tempdir().expect("tempdir");
    let directory = temp.path().join("backups");
    fs::create_dir_all(&directory).expect("backup directory");
    let backup = directory.join("Alpha.wiki");
    let content = "complete planned source\n";
    let expected_hash = compute_sha256(content);

    super::storage::publish_delete_backup_noclobber(&directory, &backup, content, &expected_hash)
        .expect("publish complete backup");
    assert_eq!(fs::read_to_string(&backup).expect("backup"), content);

    super::storage::publish_delete_backup_noclobber(&directory, &backup, content, &expected_hash)
        .expect("accept an already complete exact backup");

    fs::write(&backup, "partial").expect("replace with partial existing bytes");
    let error = super::storage::publish_delete_backup_noclobber(
        &directory,
        &backup,
        content,
        &expected_hash,
    )
    .expect_err("a mismatched existing backup must fail closed");
    assert!(error.to_string().contains("expected SHA-256"));
    assert_eq!(
        fs::read_to_string(&backup).expect("preserved existing backup"),
        "partial"
    );
    let entries = fs::read_dir(&directory)
        .expect("read backup directory")
        .map(|entry| entry.expect("backup entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec![std::ffi::OsString::from("Alpha.wiki")]);
}

#[test]
fn ambiguous_edit_reconciliation_preserves_custom_namespace_identity() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki content");
    let mut paths = paths(&project_root);
    paths.custom_namespaces.push(super::SyncNamespace {
        name: "Custom".to_string(),
        id: 3_000,
        folder: "Custom".to_string(),
    });
    let remote = RemotePage {
        title: "Custom:Alpha".to_string(),
        namespace: 3_000,
        page_id: 300,
        revision_id: 400,
        timestamp: "2026-02-19T00:00:00Z".to_string(),
        content: "custom body".to_string(),
    };
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(3_000, vec![remote.title.clone()]);
    api.page_contents
        .insert(remote.title.clone(), remote.clone());
    let mut namespaces = authoritative_namespaces();
    namespaces.push(3_000);
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces,
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed custom namespace pull");
    api.page_timestamps.insert(
        remote.title.clone(),
        PageTimestampInfo {
            title: remote.title.clone(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );
    write_file(
        &paths.wiki_content_dir.join("Custom/Alpha.wiki"),
        "custom local edit\n",
    );
    api.edit_error_after_apply = Some("injected custom namespace disconnect".to_string());
    let report = push_to_remote_with_api(
        &paths,
        &write_push_options("custom namespace ambiguous edit"),
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("ambiguous push report");
    assert!(!report.success);
    let mutation_id = report.mutation_effects[0].mutation_id;
    api.edit_error_after_apply = None;

    let reconciled = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Edit,
        mutation_id,
    )
    .expect("reconcile custom namespace edit");
    assert_eq!(
        reconciled.status,
        super::RemoteMutationReconciliationStatus::StateAdvanced
    );
    let connection = super::open_sync_connection(&paths).expect("open sync store");
    assert_eq!(
        connection
            .query_row(
                "SELECT namespace FROM sync_ledger_pages WHERE title = 'Custom:Alpha'",
                [],
                |row| row.get::<_, i32>(0),
            )
            .expect("numeric custom namespace identity"),
        3_000
    );
}

#[test]
fn authoritative_pull_retains_an_ambiguous_delete_title() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let local_before = fs::read_to_string(&source).expect("local content");
    api.delete_error_after_apply = Some("injected delete disconnect".to_string());

    let error = delete_remote_page_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "ambiguous delete before pull",
        Some(("bot", "pass")),
    )
    .expect_err("delete outcome is ambiguous");
    assert!(matches!(
        error.downcast_ref::<RemoteDeleteError>(),
        Some(RemoteDeleteError::OutcomeAmbiguous { .. })
    ));
    api.all_pages_by_namespace.insert(NS_MAIN, Vec::new());

    let pull = pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: true,
        },
        &mut api,
    )
    .expect("pull retains ambiguous delete authority");
    assert!(pull.success);
    assert!(
        pull.pages
            .iter()
            .any(|page| { page.title == "Alpha" && page.action == "retained_unresolved_mutation" })
    );
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        fs::read_to_string(source).expect("preserved source"),
        local_before
    );
}

#[test]
fn delete_reconciliation_reports_hidden_lineage_when_expected_revision_remains_current() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    api.delete_error_after_apply = Some("injected delete disconnect".to_string());

    let error = delete_remote_page_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "hidden lineage delete",
        Some(("bot", "pass")),
    )
    .expect_err("delete outcome is ambiguous");
    assert!(matches!(
        error.downcast_ref::<RemoteDeleteError>(),
        Some(RemoteDeleteError::OutcomeAmbiguous { .. })
    ));
    let mutation_id = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending mutation list")
        .mutations[0]
        .mutation_id;

    let current = base_page("Alpha", "alpha body");
    api.page_contents
        .insert(current.title.clone(), current.clone());
    api.page_timestamps.insert(
        current.title.clone(),
        PageTimestampInfo {
            title: current.title.clone(),
            timestamp: current.timestamp.clone(),
            revision_id: current.revision_id,
        },
    );
    for entry in &mut api.delete_log_entries {
        entry.comment = None;
        entry.comment_hidden = true;
    }
    api.delete_error_after_apply = None;

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("reconciliation remains unresolved");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::ReconciliationRequired
    );
    assert!(
        report
            .detail
            .contains("still matches the expected revision")
    );
    assert!(report.detail.contains("hidden delete-log comments"));
    assert!(!report.detail.contains("differs from expected revision"));
}

#[test]
fn standalone_delete_rejects_target_drift_before_any_remote_request() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let requests_before = api.request_count;
    api.target_api_url = "https://wiki-b.example/api.php".to_string();

    let error = delete_remote_page_with_api(
        &paths,
        &target("https://wiki-b.example/api.php"),
        &mut api,
        "Alpha",
        "delete target drift test",
        Some(("bot", "pass")),
    )
    .expect_err("target-bound sync store must reject retargeted delete");

    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::TargetMismatch { .. })
    ));
    assert_eq!(api.request_count, requests_before);
    assert!(api.deleted_pages.is_empty());
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
}

#[test]
fn standalone_delete_missing_credentials_is_typed_and_non_mutating() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let requests_before = api.request_count;

    let error = delete_remote_page_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "missing credentials test",
        None,
    )
    .expect_err("credentials are mandatory");

    assert!(matches!(
        error.downcast_ref::<RemoteDeleteError>(),
        Some(RemoteDeleteError::MissingCredentials)
    ));
    assert_eq!(api.request_count, requests_before);
    assert!(api.deleted_pages.is_empty());
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
}

#[test]
fn standalone_delete_reconciles_ledger_only_after_verified_delete_receipt() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let report = delete_remote_page_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "verified delete test",
        Some(("bot", "pass")),
    )
    .expect("verified delete");

    assert_eq!(report.status, super::RemoteDeleteStatus::Deleted);
    assert_eq!(report.observed_revision_id, Some(200));
    assert_eq!(report.deletion_log_id, Some(7_001));
    assert_eq!(api.deleted_pages, vec!["Alpha".to_string()]);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), (None, None));
}

#[test]
fn push_delete_treats_same_revision_postread_as_replica_lag() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let before = stored_sync_page_pair(&paths, "Alpha");
    fs::remove_file(paths.wiki_content_dir.join("Main/Alpha.wiki"))
        .expect("remove local page to plan delete");
    api.delete_postread_page = api.page_contents.get("Alpha").cloned();
    let mut options = write_push_options("replica lag delete");
    options.delete = true;

    let report = push_to_remote_with_api(&paths, &options, &mut api, Some(("bot", "pass")))
        .expect("push report");

    assert!(!report.success);
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert_eq!(
        report.mutation_effects[0].kind,
        RemoteMutationEffectKind::ReconciliationRequired
    );
    assert!(
        report.mutation_effects[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("replica lag"))
    );
}

fn exercise_missing_delete_staging_crash(fault: SyncStateFaultPoint) {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    api.page_contents.remove("Alpha");
    api.page_timestamps.remove("Alpha");
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let content = fs::read_to_string(&source).expect("local source");
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: false,
        backup_directory: None,
        backup_path: None,
        local_content_sha256: Some(compute_sha256(&content)),
    };
    let plan = super::plan_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "staging crash test",
        policy.clone(),
    )
    .expect("delete plan");
    inject_sync_state_fault_once(fault);
    super::apply_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteDeleteApplyRequest {
            title: "Alpha",
            reason: "staging crash test",
            local_effect_policy: policy,
            plan_id: Some(&plan.plan_id),
            credentials: Some(("bot", "pass")),
        },
    )
    .expect_err("injected staging crash");

    let pending = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending mutation list");
    assert_eq!(pending.mutations.len(), 1);
    let mutation = &pending.mutations[0];
    assert_eq!(mutation.operation, super::RemoteMutationOperation::Delete);
    assert_eq!(mutation.phase, "reconciliation_required");
    assert_eq!(
        mutation.local_effect_status.as_deref(),
        Some("source_staged")
    );
    assert!(!source.exists());
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), (None, None));

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation.mutation_id,
    )
    .expect("finish staged cleanup");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::StateAdvanced
    );
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation.mutation_id,
    )
    .expect("terminal receipt");
    assert_eq!(terminal.phase, "state_advanced");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
}

#[test]
fn missing_delete_recovers_crash_before_staged_source_unlink() {
    exercise_missing_delete_staging_crash(SyncStateFaultPoint::BeforeStagedSourceUnlink);
}

#[test]
fn missing_delete_recovers_crash_after_staged_source_unlink() {
    exercise_missing_delete_staging_crash(SyncStateFaultPoint::AfterStagedSourceUnlink);
}

fn source_staging_crash_fixture() -> (
    tempfile::TempDir,
    SyncProjectPaths,
    MockApi,
    i64,
    PathBuf,
    PathBuf,
) {
    let (temp, paths, mut api) = modified_alpha_fixture();
    api.page_contents.remove("Alpha");
    api.page_timestamps.remove("Alpha");
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let content = fs::read_to_string(&source).expect("local source");
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: false,
        backup_directory: None,
        backup_path: None,
        local_content_sha256: Some(compute_sha256(&content)),
    };
    let plan = super::plan_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "pre-rename ownership crash test",
        policy.clone(),
    )
    .expect("delete plan");
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterSourceRenameBeforeStagedMarker);
    super::apply_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteDeleteApplyRequest {
            title: "Alpha",
            reason: "pre-rename ownership crash test",
            local_effect_policy: policy,
            plan_id: Some(&plan.plan_id),
            credentials: Some(("bot", "pass")),
        },
    )
    .expect_err("fault after source rename");

    let pending = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending source-staging mutation");
    assert_eq!(pending.mutations.len(), 1);
    let mutation = &pending.mutations[0];
    assert_eq!(mutation.phase, "reconciliation_required");
    assert_eq!(
        mutation.local_effect_status.as_deref(),
        Some("source_staging")
    );
    assert!(!source.exists());
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
    let staging = fs::read_dir(paths.project_root.join(".wikitool/sync/delete-staging"))
        .expect("staging directory")
        .next()
        .expect("staging entry")
        .expect("read staging entry")
        .path();
    (temp, paths, api, mutation.mutation_id, source, staging)
}

fn reinsert_alpha_remote(api: &mut MockApi) {
    let current = base_page("Alpha", "alpha body");
    api.page_contents
        .insert(current.title.clone(), current.clone());
    api.page_timestamps.insert(
        current.title.clone(),
        PageTimestampInfo {
            title: current.title,
            timestamp: current.timestamp,
            revision_id: current.revision_id,
        },
    );
}

fn only_delete_recovery_file(paths: &SyncProjectPaths) -> PathBuf {
    let mut entries = fs::read_dir(paths.project_root.join(".wikitool/sync/delete-recovery"))
        .expect("delete recovery directory")
        .map(|entry| entry.expect("delete recovery entry").path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "one deterministic recovery copy");
    entries.pop().expect("recovery file")
}

#[test]
fn source_staging_crash_blocks_close_and_reconciles_owned_staging() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();

    let connection = super::open_sync_connection(&paths).expect("open sync store");
    connection
        .execute(
            "UPDATE sync_delete_mutations
             SET local_effect_status = 'backup_ready'
             WHERE mutation_id = ?1",
            [mutation_id],
        )
        .expect("simulate pre-v7 crash status");
    drop(connection);

    let close_error = super::close_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
        "Operator Example",
        "attempted closure during staging recovery",
    )
    .expect_err("operator closure cannot strand staged bytes");
    assert!(close_error.to_string().contains("in-progress or staged"));
    assert!(staging.exists());
    assert_eq!(
        super::show_remote_mutation(
            &paths,
            &default_target(),
            super::RemoteMutationOperation::Delete,
            mutation_id,
        )
        .expect("adopted legacy staging receipt")
        .local_effect_status
        .as_deref(),
        Some("source_staging")
    );

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("reconcile source-staging crash");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::StateAdvanced
    );
    assert!(!source.exists());
    assert!(!staging.exists());
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal receipt");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
}

#[test]
fn source_staging_reconciles_remote_reappearance_with_no_clobber_recovery() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    reinsert_alpha_remote(&mut api);

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("remote reappearance reconciles with exact local recovery");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::RemotePresentAfterMissing
    );
    assert_eq!(
        fs::read_to_string(&source).expect("restored exact source"),
        "alpha local edit\n"
    );
    assert!(!staging.exists());
    let recovery = only_delete_recovery_file(&paths);
    assert_eq!(
        fs::read_to_string(&recovery).expect("exact recovery copy"),
        "alpha local edit\n"
    );
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal recovery receipt");
    assert_eq!(terminal.phase, "resolved");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
    let detail = terminal.detail.as_deref().expect("durable recovery detail");
    assert!(detail.contains("no-clobber recovery copy"));
    let recovery_name = recovery
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 recovery filename");
    assert!(
        detail.contains("delete-recovery") && detail.contains(recovery_name),
        "receipt {detail:?} did not bind recovery path {}",
        recovery.display()
    );
}

#[test]
fn source_staging_recovery_is_replayable_after_crash_before_terminal_receipt() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    reinsert_alpha_remote(&mut api);
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterRetainedDeleteLocalRecovery);

    let error = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect_err("crash before terminal receipt");
    assert!(format!("{error:#}").contains("AfterRetainedDeleteLocalRecovery"));
    assert_eq!(
        fs::read_to_string(&source).expect("restored exact source"),
        "alpha local edit\n"
    );
    assert!(!staging.exists());
    let recovery = only_delete_recovery_file(&paths);
    assert_eq!(
        fs::read_to_string(&recovery).expect("durable recovery copy"),
        "alpha local edit\n"
    );
    let pending = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("pending receipt after injected crash");
    assert_eq!(pending.phase, "reconciliation_required");
    assert_eq!(
        pending.local_effect_status.as_deref(),
        Some("source_staging")
    );

    super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("idempotent recovery resumes from exact recovery copy");
    assert_eq!(
        fs::read_to_string(&source).expect("source retained after replay"),
        "alpha local edit\n"
    );
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal replay receipt");
    assert_eq!(terminal.phase, "resolved");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
    assert!(
        terminal
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("recovery copy"))
    );
}

#[test]
fn source_staging_remote_reappearance_preserves_concurrent_source() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::write(&source, "concurrent replacement\n").expect("recreate concurrent source");
    reinsert_alpha_remote(&mut api);

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("remote reappearance preserves concurrent local bytes");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::RemotePresentAfterMissing
    );
    assert_eq!(
        fs::read_to_string(&source).expect("preserved concurrent source"),
        "concurrent replacement\n"
    );
    assert!(!staging.exists());
    let recovery = only_delete_recovery_file(&paths);
    assert_eq!(
        fs::read_to_string(&recovery).expect("exact recovery copy"),
        "alpha local edit\n"
    );
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal recovery receipt");
    let detail = terminal.detail.as_deref().expect("durable recovery detail");
    assert!(detail.contains("concurrently present source"), "{detail}");
    let recovery_name = recovery
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 recovery filename");
    assert!(
        detail.contains("delete-recovery") && detail.contains(recovery_name),
        "receipt {detail:?} did not bind recovery path {}",
        recovery.display()
    );
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
}

#[test]
fn source_staging_remote_reappearance_terminalizes_with_source_already_present() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::rename(&staging, &source).expect("simulate source present before staging rename");
    reinsert_alpha_remote(&mut api);

    super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("source-present state terminalizes without staging ownership");
    assert_eq!(
        fs::read_to_string(&source).expect("retained exact source"),
        "alpha local edit\n"
    );
    assert!(!staging.exists());
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal source-present receipt");
    assert_eq!(terminal.phase, "resolved");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
    assert!(
        terminal
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("exact planned source is present"))
    );
}

#[test]
fn source_staging_remote_reappearance_records_explicit_no_backup_loss() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::remove_file(&staging).expect("simulate missing unbacked staging bytes");
    reinsert_alpha_remote(&mut api);

    super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("explicit no-backup loss terminalizes truthfully");
    assert!(!source.exists());
    assert!(!staging.exists());
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal no-backup receipt");
    assert_eq!(terminal.phase, "resolved");
    assert_eq!(terminal.local_effect_status.as_deref(), Some("complete"));
    assert!(terminal.detail.as_deref().is_some_and(|detail| {
        detail.contains("explicit no-backup policy")
            && detail.contains("no local recovery is claimed")
    }));
    assert!(stored_sync_page_pair(&paths, "Alpha").0.is_some());
}

#[test]
fn source_staging_retries_when_pre_rename_source_still_exists() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::rename(&staging, &source).expect("simulate crash before rename");

    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("retry pre-rename source staging");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::StateAdvanced
    );
    assert!(!source.exists());
    assert!(!staging.exists());
}

#[test]
fn source_staging_preserves_a_concurrently_recreated_source() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::write(&source, "concurrent replacement\n").expect("recreate source");

    super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("reconcile while preserving recreated source");
    assert_eq!(
        fs::read_to_string(&source).expect("preserved concurrent source"),
        "concurrent replacement\n"
    );
    assert!(!staging.exists());
    let terminal = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("terminal receipt");
    assert!(
        terminal.detail.as_deref().is_some_and(
            |detail| detail.contains("concurrently present local source was preserved")
        )
    );
}

#[test]
fn source_staging_without_source_or_staging_remains_truthfully_unresolved() {
    let (_temp, paths, mut api, mutation_id, source, staging) = source_staging_crash_fixture();
    fs::remove_file(&staging).expect("simulate missing staging evidence");

    let error = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect_err("missing source and staging cannot be terminalized");
    assert!(format!("{error:#}").contains("both missing"));
    assert!(!source.exists());
    assert!(!staging.exists());
    let unresolved = super::show_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("unresolved receipt");
    assert_eq!(
        unresolved.local_effect_status.as_deref(),
        Some("source_staging")
    );
    assert!(
        unresolved
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("both missing"))
    );
    super::close_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
        "Operator Example",
        "attempted closure without staging evidence",
    )
    .expect_err("closure remains forbidden without staging evidence");
}

#[test]
fn delete_reconciliation_preserves_changed_staging_instead_of_unlinking_it() {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    api.page_contents.remove("Alpha");
    api.page_timestamps.remove("Alpha");
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let content = fs::read_to_string(&source).expect("local source");
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: false,
        backup_directory: None,
        backup_path: None,
        local_content_sha256: Some(compute_sha256(&content)),
    };
    let plan = super::plan_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "staging tamper test",
        policy.clone(),
    )
    .expect("delete plan");
    inject_sync_state_fault_once(SyncStateFaultPoint::BeforeStagedSourceUnlink);
    super::apply_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteDeleteApplyRequest {
            title: "Alpha",
            reason: "staging tamper test",
            local_effect_policy: policy,
            plan_id: Some(&plan.plan_id),
            credentials: Some(("bot", "pass")),
        },
    )
    .expect_err("leave staged source for recovery");
    let staging_directory = paths.project_root.join(".wikitool/sync/delete-staging");
    let staging = fs::read_dir(&staging_directory)
        .expect("staging directory")
        .next()
        .expect("staging entry")
        .expect("read staging entry")
        .path();
    fs::write(&staging, "changed after staging\n").expect("tamper staged bytes");
    fs::write(&source, "concurrent replacement\n").expect("recreate source concurrently");
    let pending = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending mutation list");

    let error = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        pending.mutations[0].mutation_id,
    )
    .expect_err("changed staged bytes must not be unlinked");
    assert!(format!("{error:#}").contains("expected SHA-256"));
    assert_eq!(
        fs::read_to_string(&staging).expect("changed staging remains recoverable"),
        "changed after staging\n"
    );
    assert_eq!(
        fs::read_to_string(&source).expect("concurrent source remains untouched"),
        "concurrent replacement\n"
    );
}

fn exercise_delete_backup_tamper_after_request(remove_backup: bool) {
    let (_temp, paths, mut api) = modified_alpha_fixture();
    let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
    let content = fs::read_to_string(&source).expect("source content");
    let backup_directory = paths.project_root.join(".wikitool/sync/backups");
    let backup_path = backup_directory.join("Alpha-bound.wiki");
    let hook_path = backup_path.clone();
    api.delete_hook = Some(Box::new(move || {
        if remove_backup {
            fs::remove_file(&hook_path).expect("remove prepared backup");
        } else {
            fs::write(&hook_path, "tampered").expect("tamper prepared backup");
        }
    }));
    let policy = super::RemoteDeleteLocalEffectPolicy {
        backup_enabled: true,
        backup_directory: Some(backup_directory.to_string_lossy().to_string()),
        backup_path: Some(backup_path.to_string_lossy().to_string()),
        local_content_sha256: Some(compute_sha256(&content)),
    };
    let plan = super::plan_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "backup tamper test",
        policy.clone(),
    )
    .expect("plan");
    let error = super::apply_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteDeleteApplyRequest {
            title: "Alpha",
            reason: "backup tamper test",
            local_effect_policy: policy,
            plan_id: Some(&plan.plan_id),
            credentials: Some(("bot", "pass")),
        },
    )
    .expect_err("tampered backup blocks local cleanup");
    assert!(matches!(
        error.downcast_ref::<RemoteDeleteError>(),
        Some(RemoteDeleteError::ReconciliationRequired { .. })
    ));
    assert!(
        source.exists(),
        "source must remain before a valid backup exists"
    );
    assert_eq!(api.deleted_pages, vec!["Alpha".to_string()]);

    let pending = super::list_remote_mutations(&paths, &default_target(), true)
        .expect("pending mutation list");
    let mutation_id = pending.mutations[0].mutation_id;
    let close_error = super::close_remote_mutation(
        &paths,
        &default_target(),
        super::RemoteMutationOperation::Delete,
        mutation_id,
        "Operator Example",
        "attempt unsafe closure with invalid backup",
    )
    .expect_err("missing or corrupt backup must block operator closure");
    assert!(format!("{close_error:#}").contains("exact bound backup is not recoverable"));
    assert_eq!(
        super::list_remote_mutations(&paths, &default_target(), true)
            .expect("mutation remains unresolved")
            .mutations
            .len(),
        1
    );

    fs::write(&backup_path, &content).expect("restore exact backup");
    let report = super::reconcile_remote_mutation_with_api(
        &paths,
        &default_target(),
        &mut api,
        super::RemoteMutationOperation::Delete,
        mutation_id,
    )
    .expect("reconcile with restored backup");
    assert_eq!(
        report.status,
        super::RemoteMutationReconciliationStatus::StateAdvanced
    );
    assert!(!source.exists());
    assert_eq!(api.deleted_pages, vec!["Alpha".to_string()]);
}

#[test]
fn delete_reconciliation_refuses_a_disappeared_bound_backup() {
    exercise_delete_backup_tamper_after_request(true);
}

#[test]
fn delete_reconciliation_refuses_a_corrupted_bound_backup() {
    exercise_delete_backup_tamper_after_request(false);
}

#[test]
fn delete_plan_rejects_corrupt_out_of_root_cleanup_path_before_reading_it() {
    let (temp, paths, mut api) = modified_alpha_fixture();
    let outside = temp.path().join("outside.wiki");
    fs::write(&outside, "outside authority").expect("outside fixture");
    let connection = super::open_sync_connection(&paths).expect("sync store");
    connection
        .execute(
            "UPDATE sync_ledger_pages SET relative_path = '../outside.wiki' WHERE title = 'Alpha'",
            [],
        )
        .expect("corrupt ledger path");
    drop(connection);

    let error = super::plan_remote_delete_with_api(
        &paths,
        &default_target(),
        &mut api,
        "Alpha",
        "path escape test",
        super::RemoteDeleteLocalEffectPolicy {
            backup_enabled: false,
            backup_directory: None,
            backup_path: None,
            local_content_sha256: None,
        },
    )
    .expect_err("out-of-root cleanup must fail closed");
    assert!(format!("{error:#}").contains("invalid delete cleanup relative path"));
    assert_eq!(
        fs::read_to_string(&outside).expect("outside remains"),
        "outside authority"
    );
}

#[test]
fn push_rolls_back_ledger_when_snapshot_persistence_fails_after_remote_edit() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let remote = base_page("Alpha", "alpha body");
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), remote.clone());
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );
    let before = stored_sync_page_pair(&paths, "Alpha");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    accept_main_article(&paths, "Alpha");
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterLedgerUpsert);

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test transactional sync state".to_string(),
            dry_run: false,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push report");

    assert!(!report.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert_eq!(
        api.page_contents
            .get("Alpha")
            .map(|page| page.content.as_str()),
        Some("alpha local edit")
    );
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("injected sync-state transaction failure"))
    );
    let mutation = stored_edit_mutation(&paths, "Alpha");
    assert_eq!(mutation.phase, "reconciliation_required");
    assert_eq!(mutation.response_new_revision_id, Some(9_001));
}

#[test]
fn remote_deleted_modified_page_requires_force_and_recreates_with_createonly() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit after remote deletion",
    );
    accept_main_article(&paths, "Alpha");

    let blocked = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test deleted remote conflict".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("push dry run");
    assert_eq!(blocked.conflicts, vec!["Alpha".to_string()]);
    assert!(blocked.pages.iter().any(|page| {
        page.title == "Alpha"
            && page.action == "conflict"
            && page
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("deleted since last sync"))
    }));

    let recreated = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test explicit recreation".to_string(),
            dry_run: false,
            force: true,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("forced recreation");
    assert!(recreated.success);
    assert_eq!(recreated.created, 1);
    assert!(
        recreated
            .pages
            .iter()
            .any(|page| page.title == "Alpha" && page.action == "recreated")
    );
    assert_eq!(
        api.edit_constraints.last(),
        Some(&("Alpha".to_string(), EditConstraint::CreateOnly))
    );
}

#[test]
fn already_deleted_remote_cleans_local_sync_state_without_delete_request() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    fs::remove_file(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
        .expect("delete local page");

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test already deleted remote".to_string(),
            dry_run: false,
            force: false,
            delete: true,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push delete reconciliation");

    assert!(report.success);
    assert_eq!(report.unchanged, 1);
    assert!(api.deleted_pages.is_empty());
    assert!(
        report
            .pages
            .iter()
            .any(|page| page.title == "Alpha" && page.action == "already_deleted")
    );
    let plan = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection::default(),
        },
    )
    .expect("plan after cleanup")
    .expect("plan report");
    assert!(plan.changes.is_empty());
}

#[test]
fn delete_rolls_back_ledger_when_snapshot_deletion_fails_after_remote_delete() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let remote = base_page("Alpha", "alpha body");
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), remote.clone());
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );
    let before = stored_sync_page_pair(&paths, "Alpha");
    fs::remove_file(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
        .expect("delete local page");
    inject_sync_state_fault_once(SyncStateFaultPoint::AfterLedgerDelete);

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test transactional sync-state delete".to_string(),
            dry_run: false,
            force: false,
            delete: true,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push delete report");

    assert!(!report.success);
    assert_eq!(api.deleted_pages, vec!["Alpha".to_string()]);
    assert!(!api.page_contents.contains_key("Alpha"));
    assert_eq!(stored_sync_page_pair(&paths, "Alpha"), before);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("injected sync-state transaction failure"))
    );
}

#[test]
fn delete_rechecks_revision_immediately_and_refuses_same_run_race() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    fs::remove_file(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
        .expect("delete local page");
    api.timestamp_responses = vec![
        vec![PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            revision_id: 200,
        }],
        // Applying a preview must first observe the same remote revision bound
        // into the plan. The destructive-operation recheck below is the race.
        vec![PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            revision_id: 200,
        }],
        vec![PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:01Z".to_string(),
            revision_id: 201,
        }],
    ];

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test delete revision race".to_string(),
            dry_run: false,
            force: false,
            delete: true,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push delete race");

    assert!(!report.success);
    assert_eq!(report.conflicts, vec!["Alpha".to_string()]);
    assert!(api.deleted_pages.is_empty());
    assert_eq!(api.timestamp_batches.len(), 3);
    assert!(report.pages.iter().any(|page| {
        page.title == "Alpha"
            && page.action == "conflict"
            && page
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("changed during push planning"))
    }));
}

#[test]
fn push_dry_run_requires_publication_preflight_even_with_force() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "unaccepted local prose",
    );

    let push_target = target(api.target_api_url());
    let report = super::push_to_remote_with_api_and_preflight(
        &paths,
        &PushOptions {
            summary: "test unaccepted dry run".to_string(),
            dry_run: true,
            force: true,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &push_target,
        &mut api,
        None,
        &RejectingPublicationPreflight,
    )
    .expect("push dry run");

    assert!(!report.success);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("publication preflight failed"))
    );
    assert!(
        report.pages.iter().any(|page| {
            page.title == "Alpha" && page.action == "blocked_publication_preflight"
        })
    );
    assert!(api.edited_pages.is_empty());
}

#[test]
fn sync_transport_delegates_main_publication_policy_to_explicit_preflight() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
    let paths = paths(&project_root);
    let remote = base_page("Alpha", "alpha body");
    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), remote.clone());
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: remote.timestamp,
            revision_id: remote.revision_id,
        },
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );

    let push_target = target(api.target_api_url());
    let mut options = PushOptions {
        summary: "test explicit publication policy".to_string(),
        dry_run: true,
        force: false,
        delete: false,
        include_templates: false,
        categories_only: false,
        all: true,
        selection: SyncSelection::default(),
        apply_plan_id: None,
    };
    let preview = super::push::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        None,
        &PassThroughPublicationPreflight,
    )
    .expect("generic sync preview");
    options.dry_run = false;
    options.apply_plan_id = preview.plan_id;
    let report = super::push::push_to_remote_with_api_and_preflight(
        &paths,
        &options,
        &push_target,
        &mut api,
        Some(("bot", "pass")),
        &PassThroughPublicationPreflight,
    )
    .expect("generic sync push");

    assert!(report.success);
    assert_eq!(api.edited_pages, vec!["Alpha".to_string()]);
    assert!(report.pages.iter().all(|page| page.acceptance.is_none()));
}

#[test]
fn push_detects_remote_conflict_without_force() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi {
        login_required: true,
        ..Default::default()
    };
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    accept_main_article(&paths, "Alpha");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-22T00:00:00Z".to_string(),
            revision_id: 9999,
        },
    );

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test conflict".to_string(),
            dry_run: false,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        Some(("bot", "pass")),
    )
    .expect("push");

    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0], "Alpha");
    assert!(api.edited_pages.is_empty());
}

#[test]
fn force_cannot_reuse_coincident_revision_identity_across_wiki_targets() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");

    let mut source_api = MockApi::default();
    source_api
        .all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    source_api
        .page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut source_api,
    )
    .expect("seed source-wiki baseline");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    accept_main_article(&paths, "Alpha");

    let mut other_api = MockApi {
        target_api_url: "https://wiki-b.example/api.php".to_string(),
        ..Default::default()
    };
    other_api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            revision_id: 200,
        },
    );

    let error = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "must not cross wiki targets".to_string(),
            dry_run: false,
            force: true,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut other_api,
        Some(("bot", "pass")),
    )
    .expect_err("target mismatch must fail before force or revision comparison");

    assert!(matches!(
        error.downcast_ref::<SyncStateError>(),
        Some(SyncStateError::TargetMismatch {
            stored_api_url,
            requested_api_url,
            ..
        }) if stored_api_url == "https://wiki-a.example/api.php"
            && requested_api_url == "https://wiki-b.example/api.php"
    ));
    assert_eq!(other_api.request_count, 0);
    assert!(other_api.edited_pages.is_empty());
}

#[test]
fn push_dry_run_detects_remote_conflict_without_writes() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    accept_main_article(&paths, "Alpha");
    api.page_timestamps.insert(
        "Alpha".to_string(),
        PageTimestampInfo {
            title: "Alpha".to_string(),
            timestamp: "2026-02-22T00:00:00Z".to_string(),
            revision_id: 9999,
        },
    );

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test dry-run conflict".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("push dry run");

    assert!(report.dry_run);
    assert_eq!(report.conflicts, vec!["Alpha".to_string()]);
    assert!(api.edited_pages.is_empty());
}

#[test]
fn forced_push_dry_run_still_hydrates_remote_revision_identity() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.timestamp_batches.clear();

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha forced local edit",
    );
    accept_main_article(&paths, "Alpha");

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test forced dry-run".to_string(),
            dry_run: true,
            force: true,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("forced push dry run");

    assert!(report.dry_run);
    assert!(report.conflicts.is_empty());
    assert_eq!(api.timestamp_batches, vec![vec!["Alpha".to_string()]]);
    assert!(api.edited_pages.is_empty());
}

#[test]
fn push_conflict_hydration_fetches_changed_titles_in_one_batch() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    api.page_contents
        .insert("Beta".to_string(), base_page("Beta", "beta body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");
    api.timestamp_batches.clear();

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "beta local edit",
    );
    accept_main_article(&paths, "Alpha");
    accept_main_article(&paths, "Beta");

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test batched dry-run".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            all: true,
            selection: SyncSelection::default(),
            apply_plan_id: None,
        },
        &mut api,
        None,
    )
    .expect("push dry run");

    assert!(report.dry_run);
    assert_eq!(api.timestamp_batches.len(), 1);
    assert_eq!(
        api.timestamp_batches[0],
        vec!["Alpha".to_string(), "Beta".to_string()]
    );
}

#[test]
fn diff_content_uses_snapshots_and_reports_missing_baseline() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    api.page_contents
        .insert("Beta".to_string(), base_page("Beta", "beta body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );

    let diff = diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: false,
            categories_only: false,
            include_content: true,
            selection: SyncSelection::default(),
        },
    )
    .expect("diff")
    .expect("diff report");
    let alpha = diff
        .changes
        .iter()
        .find(|change| change.title == "Alpha")
        .expect("alpha diff");
    assert_eq!(alpha.baseline_status, Some(DiffBaselineStatus::Available));
    assert!(
        alpha
            .unified_diff
            .as_deref()
            .is_some_and(|diff| diff.contains("-alpha body") && diff.contains("+alpha local edit"))
    );

    let connection = super::open_sync_connection(&paths).expect("open sync db");
    connection
        .execute("DELETE FROM sync_snapshots WHERE title = 'Alpha'", [])
        .expect("delete snapshot");

    let diff = diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: false,
            categories_only: false,
            include_content: true,
            selection: SyncSelection::default(),
        },
    )
    .expect("diff after snapshot delete")
    .expect("diff report");
    let alpha = diff
        .changes
        .iter()
        .find(|change| change.title == "Alpha")
        .expect("alpha diff");
    assert_eq!(
        alpha.baseline_status,
        Some(DiffBaselineStatus::MissingSnapshot)
    );
    assert!(alpha.unified_diff.is_none());
}

#[test]
fn sync_plan_selection_and_changed_article_paths_honor_scope() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(test_state_dir(&paths)).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace.insert(
        NS_MAIN,
        vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()],
    );
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    api.page_contents
        .insert("Beta".to_string(), base_page("Beta", "beta body"));
    api.page_contents
        .insert("Gamma".to_string(), base_page("Gamma", "gamma body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: authoritative_namespaces(),
            category: None,
            full: true,
            coverage: PullCoverage::GlobalAllNamespaces,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "#REDIRECT [[Alpha]]",
    );

    let selected = plan_sync_changes(
        &paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: SyncSelection {
                titles: vec!["Alpha".to_string()],
                paths: Vec::new(),
            },
        },
    )
    .expect("plan selection")
    .expect("plan report");
    assert_eq!(selected.changes.len(), 1);
    assert_eq!(selected.changes[0].title, "Alpha");

    let changed_paths = collect_changed_article_paths(&paths, &SyncSelection::default(), false)
        .expect("collect changed paths")
        .expect("changed paths");
    assert_eq!(
        changed_paths,
        vec!["wiki_content/Main/Alpha.wiki".to_string()]
    );

    let selected_redirect_paths = collect_changed_article_paths(
        &paths,
        &SyncSelection {
            titles: vec!["Beta".to_string()],
            paths: Vec::new(),
        },
        true,
    )
    .expect("collect changed paths with redirect")
    .expect("changed paths");
    assert_eq!(
        selected_redirect_paths,
        vec!["wiki_content/Main/Beta.wiki".to_string()]
    );
}
