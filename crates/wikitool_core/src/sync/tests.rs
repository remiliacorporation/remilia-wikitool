use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::json;
use tempfile::tempdir;

use super::{
    DiffBaselineStatus, DiffChangeType, DiffOptions, ExternalSearchHit, NS_MAIN, PageTimestampInfo,
    PullOptions, PushOptions, RemotePage, SiteInfoNamespace, SyncPlanOptions, SyncSelection,
    WikiReadApi, WikiWriteApi, collect_changed_article_paths, diff_local_against_sync,
    namespace_display_name, plan_sync_changes, pull_from_remote_with_api, push_to_remote_with_api,
    should_include_discovered_namespace,
};
use crate::runtime::{ResolvedPaths, ValueSource};

#[derive(Default)]
struct MockApi {
    all_pages_by_namespace: BTreeMap<i32, Vec<String>>,
    recent_changes: Vec<String>,
    category_members: Vec<String>,
    page_contents: BTreeMap<String, RemotePage>,
    page_timestamps: BTreeMap<String, PageTimestampInfo>,
    search_hits: Vec<ExternalSearchHit>,
    edited_pages: Vec<String>,
    deleted_pages: Vec<String>,
    login_required: bool,
    logged_in: bool,
    request_count: usize,
}

impl WikiReadApi for MockApi {
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

    fn search(
        &mut self,
        _query: &str,
        _namespaces: &[i32],
        _limit: usize,
    ) -> anyhow::Result<Vec<ExternalSearchHit>> {
        self.request_count += 1;
        Ok(self.search_hits.clone())
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
        _summary: &str,
    ) -> anyhow::Result<RemotePage> {
        self.request_count += 1;
        if self.login_required && !self.logged_in {
            anyhow::bail!("not logged in");
        }
        self.edited_pages.push(title.to_string());
        let page = RemotePage {
            title: title.to_string(),
            namespace: NS_MAIN,
            page_id: 9000,
            revision_id: 9001,
            timestamp: "2026-02-20T00:00:00Z".to_string(),
            content: content.to_string(),
        };
        self.page_contents.insert(title.to_string(), page.clone());
        self.page_timestamps.insert(
            title.to_string(),
            PageTimestampInfo {
                title: title.to_string(),
                timestamp: page.timestamp.clone(),
                revision_id: page.revision_id,
            },
        );
        Ok(page)
    }

    fn delete_page(&mut self, title: &str, _reason: &str) -> anyhow::Result<()> {
        self.request_count += 1;
        if self.login_required && !self.logged_in {
            anyhow::bail!("not logged in");
        }
        self.deleted_pages.push(title.to_string());
        self.page_timestamps.remove(title);
        self.page_contents.remove(title);
        Ok(())
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write file");
}

fn paths(project_root: &Path) -> ResolvedPaths {
    ResolvedPaths {
        project_root: project_root.to_path_buf(),
        wiki_content_dir: project_root.join("wiki_content"),
        templates_dir: project_root.join("templates"),
        state_dir: project_root.join(".wikitool"),
        data_dir: project_root.join(".wikitool").join("data"),
        db_path: project_root
            .join(".wikitool")
            .join("data")
            .join("wikitool.db"),
        config_path: project_root.join(".wikitool").join("config.toml"),
        parser_config_path: project_root
            .join(".wikitool")
            .join(crate::runtime::PARSER_CONFIG_FILENAME),
        root_source: ValueSource::Flag,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    }
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

#[test]
fn namespace_discovery_filters_builtin_and_talk_namespaces() {
    let builtin = SiteInfoNamespace {
        id: 14,
        canonical: Some("Category".to_string()),
        name: Some("Category".to_string()),
        star_name: Some("Category".to_string()),
        content: Some(json!(true)),
    };
    let talk = SiteInfoNamespace {
        id: 3001,
        canonical: Some("Lore talk".to_string()),
        name: Some("Lore talk".to_string()),
        star_name: Some("Lore talk".to_string()),
        content: Some(json!(false)),
    };
    assert!(!should_include_discovered_namespace(&builtin));
    assert!(!should_include_discovered_namespace(&talk));
}

#[test]
fn namespace_discovery_includes_custom_content_namespace() {
    let custom = SiteInfoNamespace {
        id: 3000,
        canonical: Some("Lore".to_string()),
        name: Some("Lore".to_string()),
        star_name: Some("Lore".to_string()),
        content: Some(json!(true)),
    };
    assert!(should_include_discovered_namespace(&custom));
}

#[test]
fn namespace_display_name_prefers_canonical_and_normalizes_underscores() {
    let namespace = SiteInfoNamespace {
        id: 3000,
        canonical: Some("Lore_Namespace".to_string()),
        name: Some("Ignored".to_string()),
        star_name: Some("Ignored".to_string()),
        content: Some(json!(true)),
    };
    assert_eq!(
        namespace_display_name(&namespace).as_deref(),
        Some("Lore Namespace")
    );
}

#[test]
fn pull_writes_files_and_reindexes() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
    assert!(report.reindex.is_some());
}

#[test]
fn pull_skips_modified_local_when_overwrite_is_disabled() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("pull");

    assert_eq!(report.created, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 1);
    let current = fs::read_to_string(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
        .expect("read local file");
    assert_eq!(current, "local edited");
}

#[test]
fn incremental_pull_does_not_advance_checkpoint_when_page_is_skipped() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
    fs::create_dir_all(&paths.state_dir).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
fn diff_detects_new_modified_and_deleted_local_pages() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
    fs::create_dir_all(&paths.state_dir).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "gamma local",
    );

    let report = push_to_remote_with_api(
        &paths,
        &PushOptions {
            summary: "test dry run".to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: false,
            categories_only: false,
            selection: SyncSelection::default(),
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
}

#[test]
fn push_detects_remote_conflict_without_force() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
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
            selection: SyncSelection::default(),
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
fn push_dry_run_detects_remote_conflict_without_writes() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

    let mut api = MockApi::default();
    api.all_pages_by_namespace
        .insert(NS_MAIN, vec!["Alpha".to_string()]);
    api.page_contents
        .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

    pull_from_remote_with_api(
        &paths,
        &PullOptions {
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
            overwrite_local: false,
        },
        &mut api,
    )
    .expect("seed pull");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "alpha local edit",
    );
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
            selection: SyncSelection::default(),
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
fn diff_content_uses_snapshots_and_reports_missing_baseline() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
    fs::create_dir_all(&paths.state_dir).expect("create state");

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
            namespaces: vec![NS_MAIN],
            category: None,
            full: true,
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
