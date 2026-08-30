use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;
use wikitool_core::{config::load_config, site::site_adapter_resource_paths};

use crate::cli_support::{OutputFormat, normalize_path};

use super::reference::{generate_docs_reference_markdown, source_repo_root};

#[derive(Debug, Args)]
pub(crate) struct DocsAuditArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root whose skill routes and site-adapter selection should be audited"
    )]
    pub(crate) host_project_root: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct DocsAuditReport {
    schema_version: &'static str,
    status: &'static str,
    repo_root: String,
    host_project_root: Option<String>,
    check_count: usize,
    failure_count: usize,
    checks: Vec<DocsAuditCheck>,
}

#[derive(Debug, Serialize)]
struct DocsAuditCheck {
    id: &'static str,
    status: &'static str,
    path: Option<String>,
    message: String,
}

pub(crate) fn run_docs_audit(args: DocsAuditArgs) -> Result<()> {
    let repo_root = source_repo_root()?;
    let host_project_root = args
        .host_project_root
        .as_ref()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .context("failed to resolve current directory")?
                    .join(path)
            }
            .canonicalize()
            .context("failed to resolve host project root")
        })
        .transpose()?;

    let mut checks = Vec::new();
    audit_reference(&repo_root, &mut checks);
    audit_default_features(&repo_root, &mut checks);
    audit_skills_layout(&repo_root, &mut checks);
    audit_canonical_skills(&repo_root, &mut checks);
    audit_source_root_skill_routes(&repo_root, &mut checks);
    audit_generic_boundary(&repo_root, &mut checks);
    audit_no_retired_public_terms(&repo_root, &mut checks);
    if let Some(host_root) = host_project_root.as_ref() {
        audit_host_project(host_root, &mut checks);
    }

    let failure_count = checks.iter().filter(|check| check.status == "fail").count();
    let report = DocsAuditReport {
        schema_version: "docs_audit_v2",
        status: if failure_count == 0 { "pass" } else { "fail" },
        repo_root: normalize_path(&repo_root),
        host_project_root: host_project_root.as_ref().map(normalize_path),
        check_count: checks.len(),
        failure_count,
        checks,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_docs_audit_report(&report);
    }

    if report.failure_count == 0 {
        Ok(())
    } else {
        bail!("docs audit failed with {} failure(s)", report.failure_count)
    }
}

fn audit_reference(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let path = repo_root.join("docs/wikitool/reference.md");
    let actual = read_to_string(&path);
    let expected = generate_docs_reference_markdown();
    let ok = matches!((&actual, &expected), (Ok(left), Ok(right)) if normalize_newlines(left) == normalize_newlines(right));
    let message = match (actual, expected) {
        (Ok(_), Ok(_)) if ok => "generated CLI reference is current".to_string(),
        (Ok(_), Ok(_)) => {
            "generated CLI reference is stale; run `cargo run --package wikitool --features maintainer -- docs generate-reference`".to_string()
        }
        (Err(error), _) => format!("failed to read generated reference: {error}"),
        (_, Err(error)) => format!("failed to render generated reference: {error}"),
    };
    push_check(checks, "reference.generated", ok, Some(&path), message);
}

fn audit_default_features(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let path = repo_root.join("crates/wikitool/Cargo.toml");
    match read_to_string(&path) {
        Ok(body) => {
            let ok = body.lines().any(|line| line.trim() == "default = []");
            push_check(
                checks,
                "cargo.default_surface",
                ok,
                Some(&path),
                if ok {
                    "normal builds use the end-user surface".to_string()
                } else {
                    "Cargo default features must stay empty".to_string()
                },
            );
        }
        Err(error) => push_check(
            checks,
            "cargo.default_surface",
            false,
            Some(&path),
            format!("failed to read Cargo.toml: {error}"),
        ),
    }
}

fn audit_skills_layout(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    for relative in [
        "docs/wikitool/skill-integration.md",
        "docs/wikitool/site-adapters.md",
        "site_adapters/generic/site-adapter.toml",
        "site_adapters/remilia-wiki/site-adapter.toml",
    ] {
        let path = repo_root.join(relative);
        push_check(
            checks,
            "skills.layered_layout",
            path.is_file(),
            Some(&path),
            if path.is_file() {
                format!("{relative} is present")
            } else {
                format!("required skills or adapter resource is missing: {relative}")
            },
        );
    }

    let retired = repo_root.join("agent-pack");
    push_check(
        checks,
        "skills.no_legacy_pack",
        !retired.exists(),
        Some(&retired),
        if retired.exists() {
            "retired agent-pack source must stay removed".to_string()
        } else {
            "canonical skills have no legacy agent-pack authority".to_string()
        },
    );
}

fn audit_canonical_skills(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    for (name, references) in [
        (
            "wiki-writing",
            &[
                "evidence-to-prose.md",
                "human-notes.md",
                "mediawiki-structure.md",
            ][..],
        ),
        (
            "prose-review",
            &["source-fidelity.md", "reader-value.md", "blp-sensitive.md"][..],
        ),
        ("wiki-interview", &["interview-ledger.md"][..]),
        ("wikitool", &[][..]),
    ] {
        let root = repo_root.join(".agents/skills").join(name);
        let skill_path = root.join("SKILL.md");
        let mut failures = Vec::new();
        match read_to_string(&skill_path) {
            Ok(body) => {
                if !valid_skill_frontmatter(&body, name) {
                    failures.push("invalid name/description-only frontmatter".to_string());
                }
                if !body.contains("## Procedure") || !body.contains("## Exit conditions") {
                    failures.push("missing procedure or exit conditions".to_string());
                }
                for reference in references {
                    if !body.contains(reference)
                        || !root.join("references").join(reference).is_file()
                    {
                        failures.push(format!("missing routed reference {reference}"));
                    }
                }
            }
            Err(error) => failures.push(format!("failed to read SKILL.md: {error}")),
        }
        if !root.join("agents/openai.yaml").is_file() {
            failures.push("missing agents/openai.yaml".to_string());
        }
        push_check(
            checks,
            "skills.canonical_shape",
            failures.is_empty(),
            Some(&skill_path),
            if failures.is_empty() {
                format!("{name} has a substantive canonical skill package")
            } else {
                format!("{name}: {}", failures.join("; "))
            },
        );
    }
}

fn audit_source_root_skill_routes(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let directory = repo_root.join(".claude/skills/wikitool");
    let path = directory.join("SKILL.md");
    let directory = path.parent().expect("source-root skill directory");
    let canonical = ".agents/skills/wikitool/SKILL.md";
    match read_to_string(&path) {
        Ok(body) => {
            let line_count = body.lines().count();
            let only_canonical_entrypoint = std::fs::read_dir(directory).is_ok_and(|entries| {
                let mut names = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name())
                    .collect::<Vec<_>>();
                names.sort();
                names == ["SKILL.md"]
            });
            let routes_to_canonical =
                body.contains(canonical) && line_count <= 16 && only_canonical_entrypoint;
            push_check(
                checks,
                "skills.source_root_route",
                routes_to_canonical,
                Some(&path),
                if routes_to_canonical {
                    "source-root Claude entrypoint is a thin canonical route".to_string()
                } else {
                    format!(
                        "source-root Claude entrypoint must route to {canonical} without a parallel command tree"
                    )
                },
            );
        }
        Err(error) => push_check(
            checks,
            "skills.source_root_route",
            false,
            Some(&path),
            format!("failed to read source-root Claude entrypoint: {error}"),
        ),
    }

    let entries = std::fs::read_dir(directory).and_then(|entries| {
        entries
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()
    });
    match entries {
        Ok(entries) => {
            let single_entrypoint = entries.len() == 1 && entries[0] == "SKILL.md";
            push_check(
                checks,
                "skills.source_root_single_entrypoint",
                single_entrypoint,
                Some(directory),
                if single_entrypoint {
                    "source-root Claude adapter contains only its canonical entrypoint".to_string()
                } else {
                    format!(
                        "source-root Claude adapter must contain only SKILL.md, found: {}",
                        entries
                            .iter()
                            .map(|entry| entry.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            );
        }
        Err(error) => push_check(
            checks,
            "skills.source_root_single_entrypoint",
            false,
            Some(directory),
            format!("failed to inspect source-root Claude adapter: {error}"),
        ),
    }
}

fn valid_skill_frontmatter(body: &str, expected_name: &str) -> bool {
    let mut lines = body.lines();
    if lines.next() != Some("---") {
        return false;
    }
    let mut fields = Vec::new();
    for line in &mut lines {
        if line == "---" {
            break;
        }
        if !line.trim().is_empty() {
            fields.push(line);
        }
    }
    fields.len() == 2
        && fields
            .iter()
            .any(|line| *line == format!("name: {expected_name}"))
        && fields.iter().any(|line| line.starts_with("description: "))
}

fn audit_generic_boundary(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let root = repo_root.join(".agents/skills");
    let mut leaks = Vec::new();
    for path in text_files(&root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        let lowered = body.to_ascii_lowercase();
        for token in [
            "remilia",
            "charlotte fang",
            "milady maker",
            "wiki.remilia.org",
            "d3chart",
        ] {
            if lowered.contains(token) {
                leaks.push(format!("{} contains {token:?}", normalize_path(&path)));
            }
        }
    }
    push_check(
        checks,
        "skills.target_neutral",
        leaks.is_empty(),
        Some(&root),
        if leaks.is_empty() {
            "generic skill guidance contains no target-specific policy".to_string()
        } else {
            leaks.join("; ")
        },
    );
}

fn audit_no_retired_public_terms(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let mut failures = Vec::new();
    for path in markdown_files(repo_root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        if path.file_name().is_some_and(|name| name == "CHANGELOG.md") {
            continue;
        }
        let lowered = body.to_ascii_lowercase();
        for term in [
            "wikitool search",
            "wikitool fetch",
            "wikitool seo",
            "wikitool net",
            "--view agent-card",
            "function-card",
            "function-context",
            "minibeast",
            "agent-pack",
            "agent pack",
            "wikitool agent",
            "wikitool-operator",
            ".wikitool-agent",
        ] {
            if lowered.contains(term) {
                failures.push(format!("{} contains `{term}`", normalize_path(&path)));
            }
        }
    }
    push_check(
        checks,
        "guidance.no_retired_surface",
        failures.is_empty(),
        Some(repo_root),
        if failures.is_empty() {
            "guidance does not mention retired or private public surfaces".to_string()
        } else {
            failures.join("; ")
        },
    );
}

fn audit_host_project(host_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let (adapter_ok, adapter, adapter_message) = inspect_host_site_adapter(host_root);
    push_check(
        checks,
        "host.site_adapter",
        adapter_ok,
        Some(&adapter),
        adapter_message,
    );

    for relative in [
        ".agents/skills/wikitool/SKILL.md",
        ".agents/skills/wiki-writing/SKILL.md",
        ".agents/skills/prose-review/SKILL.md",
        ".agents/skills/wiki-interview/SKILL.md",
        ".claude/skills/wikitool/SKILL.md",
        ".claude/skills/wiki-writing/SKILL.md",
        ".claude/skills/prose-review/SKILL.md",
        ".claude/skills/wiki-interview/SKILL.md",
    ] {
        let path = host_root.join(relative);
        let ok = read_to_string(&path).is_ok_and(|body| {
            body.contains("tools/wikitool/.agents/skills/") && !body.contains("wikitool search")
        });
        push_check(
            checks,
            "host.skill_redirects",
            ok,
            Some(&path),
            if ok {
                format!("{relative} routes to the public Wikitool skills")
            } else {
                format!("{relative} must route to the public Wikitool skills")
            },
        );
    }

    for relative in [
        ".claude/skills/review.md",
        ".claude/skills/knowledge-interview.md",
    ] {
        let path = host_root.join(relative);
        push_check(
            checks,
            "host.retired_skill_aliases",
            !path.exists(),
            Some(&path),
            if path.exists() {
                format!("retired skill alias {relative} must be removed")
            } else {
                format!("retired skill alias {relative} is absent")
            },
        );
    }

    for relative in [
        ".claude/skills/wikitool.md",
        ".claude/skills/wiki-writing.md",
        ".claude/skills/prose-review.md",
        ".claude/skills/wiki-interview.md",
    ] {
        let path = host_root.join(relative);
        push_check(
            checks,
            "host.no_legacy_flat_skill_wrappers",
            !path.exists(),
            Some(&path),
            if path.exists() {
                format!("legacy flat Claude skill wrapper must be migrated: {relative}")
            } else {
                format!("legacy flat Claude skill wrapper is absent: {relative}")
            },
        );
    }
}

fn inspect_host_site_adapter(host_root: &Path) -> (bool, PathBuf, String) {
    let config_path = host_root.join(".wikitool/config.toml");
    let conventional_adapter = host_root.join("wikitool_adapter/site-adapter.toml");
    let config = match load_config(&config_path) {
        Ok(config) => config,
        Err(error) => {
            return (
                false,
                config_path,
                format!("failed to load host runtime configuration: {error:#}"),
            );
        }
    };

    let Some(configured) = config.adapter.path.as_deref() else {
        if conventional_adapter.exists() {
            return validate_host_adapter(
                host_root,
                conventional_adapter,
                "host release supplement",
            );
        }
        return (
            true,
            config_path,
            "host uses Wikitool's embedded generic adapter and owns no private release supplement"
                .to_string(),
        );
    };

    let configured = configured.trim();
    if configured.is_empty() {
        return (
            false,
            config_path,
            "host adapter.path must not be empty".to_string(),
        );
    }
    let configured_path = Path::new(configured);
    if configured_path.is_absolute() {
        return (
            false,
            configured_path.to_path_buf(),
            "host adapter.path must be project-relative".to_string(),
        );
    }
    validate_host_adapter(
        host_root,
        host_root.join(configured_path),
        "configured host adapter",
    )
}

fn validate_host_adapter(
    host_root: &Path,
    adapter: PathBuf,
    label: &str,
) -> (bool, PathBuf, String) {
    let canonical_host = match std::fs::canonicalize(host_root) {
        Ok(path) => path,
        Err(error) => {
            return (
                false,
                host_root.to_path_buf(),
                format!("failed to resolve host project root: {error}"),
            );
        }
    };
    let canonical_adapter = match std::fs::canonicalize(&adapter) {
        Ok(path) => path,
        Err(error) => {
            return (
                false,
                adapter,
                format!("failed to resolve {label}: {error}"),
            );
        }
    };
    if !canonical_adapter.starts_with(&canonical_host) {
        return (
            false,
            canonical_adapter,
            format!("{label} resolves outside the host project root"),
        );
    }
    match site_adapter_resource_paths(&canonical_adapter) {
        Ok(resources) => (
            true,
            canonical_adapter,
            format!(
                "{label} is valid and declares {} resource(s)",
                resources.len()
            ),
        ),
        Err(error) => (
            false,
            canonical_adapter,
            format!("{label} is invalid: {error:#}"),
        ),
    }
}

fn text_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_files(root, &mut out, &["md", "yaml", "toml"]);
    out
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_files(root, &mut out, &["md"]);
    out
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>, extensions: &[&str]) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if !matches!(name, ".git" | "target" | ".wikitool" | ".wikitest" | "dist") {
                collect_files(&path, out, extensions);
            }
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extensions.iter().any(|item| item == &extension))
        {
            out.push(path);
        }
    }
}

fn push_check(
    checks: &mut Vec<DocsAuditCheck>,
    id: &'static str,
    ok: bool,
    path: Option<&Path>,
    message: String,
) {
    checks.push(DocsAuditCheck {
        id,
        status: if ok { "pass" } else { "fail" },
        path: path.map(normalize_path),
        message,
    });
}

fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

fn print_docs_audit_report(report: &DocsAuditReport) {
    println!("docs audit");
    println!("status: {}", report.status);
    println!("repo_root: {}", report.repo_root);
    if let Some(host) = &report.host_project_root {
        println!("host_project_root: {host}");
    }
    println!("check_count: {}", report.check_count);
    println!("failure_count: {}", report.failure_count);
    for check in &report.checks {
        println!(
            "check: id={} status={} path={} message={}",
            check.id,
            check.status,
            check.path.as_deref().unwrap_or("<none>"),
            check.message
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{inspect_host_site_adapter, valid_skill_frontmatter};

    #[test]
    fn skill_frontmatter_rejects_extra_policy_keys() {
        assert!(valid_skill_frontmatter(
            "---\nname: demo\ndescription: A useful demo skill.\n---\n",
            "demo"
        ));
        assert!(!valid_skill_frontmatter(
            "---\nname: demo\ndescription: A useful demo skill.\nallowed-tools: Bash\n---\n",
            "demo"
        ));
    }

    #[test]
    fn host_adapter_audit_accepts_embedded_generic_default() {
        let host = tempfile::tempdir().expect("host");
        let (ok, _, message) = inspect_host_site_adapter(host.path());
        assert!(ok);
        assert!(message.contains("embedded generic adapter"));
    }

    #[test]
    fn host_adapter_audit_validates_configured_bundled_adapter() {
        let host = tempfile::tempdir().expect("host");
        let state = host.path().join(".wikitool");
        let adapter = host
            .path()
            .join("tools/wikitool/site_adapters/remilia-wiki");
        fs::create_dir_all(&state).expect("state directory");
        fs::create_dir_all(&adapter).expect("adapter directory");
        fs::write(
            state.join("config.toml"),
            "[adapter]\npath = \"tools/wikitool/site_adapters/remilia-wiki/site-adapter.toml\"\n",
        )
        .expect("config");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../site_adapters/generic/site-adapter.toml"),
            adapter.join("site-adapter.toml"),
        )
        .expect("adapter");

        let (ok, path, message) = inspect_host_site_adapter(host.path());
        assert!(ok, "{message}");
        assert_eq!(
            path,
            adapter
                .join("site-adapter.toml")
                .canonicalize()
                .expect("canonical adapter")
        );
        assert!(message.contains("configured host adapter is valid"));
    }

    #[test]
    fn host_adapter_audit_rejects_invalid_selected_adapter() {
        let host = tempfile::tempdir().expect("host");
        let state = host.path().join(".wikitool");
        let adapter = host.path().join("site-adapter");
        fs::create_dir_all(&state).expect("state directory");
        fs::create_dir_all(&adapter).expect("adapter directory");
        fs::write(
            state.join("config.toml"),
            "[adapter]\npath = \"site-adapter/site-adapter.toml\"\n",
        )
        .expect("config");
        fs::write(adapter.join("site-adapter.toml"), "not valid toml = [").expect("adapter");

        let (ok, _, message) = inspect_host_site_adapter(host.path());
        assert!(!ok);
        assert!(message.contains("configured host adapter is invalid"));
    }
}
