//! Read-only migration evidence from current local bytes, independent of catalog freshness.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::content_store::parsing::{
    canonical_template_title, scan_template_body_ranges, split_once_top_level_equals,
    split_template_segment_ranges,
};
use crate::filesystem::{ScanOptions, scan_files, validate_scoped_path};
use crate::runtime::ResolvedPaths;
use crate::support::compute_sha256;

const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateMigrationSpec {
    pub schema: String,
    pub from_template: String,
    pub to_template: String,
    pub title_case: MigrationTitleCase,
    #[serde(default)]
    pub parameter_renames: BTreeMap<String, String>,
    /// These parameters require an editorial decision; the planner never drops their values.
    #[serde(default)]
    pub deprecated_parameters: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTitleCase {
    FirstLetter,
    CaseSensitive,
}

impl TemplateMigrationSpec {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "template_migration_spec_v1" {
            bail!("unsupported template migration schema: {}", self.schema);
        }
        for title in [&self.from_template, &self.to_template] {
            if title.len() > 512
                || template_identity(title, self.title_case).is_none()
                || title.contains(['|', '<', '>', '\n', '\r'])
            {
                bail!("migration requires a literal Template title: {title}");
            }
        }
        if self.parameter_renames.len() > 128 || self.deprecated_parameters.len() > 128 {
            bail!("migration supports at most 128 parameter renames and deprecations");
        }
        for key in self
            .parameter_renames
            .keys()
            .chain(self.parameter_renames.values())
            .chain(self.deprecated_parameters.iter())
        {
            if key.is_empty()
                || key.len() > 256
                || key.trim() != key
                || key.contains(['|', '=', '{', '}', '<', '>', '\n', '\r'])
            {
                bail!(
                    "migration requires literal nonblank parameter names without template syntax"
                );
            }
        }
        if self.parameter_renames.iter().any(|(from, to)| from == to) {
            bail!("migration parameter rename must change its name");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct TemplateMigrationPlan {
    pub schema: &'static str,
    pub plan_id: String,
    pub spec: TemplateMigrationSpec,
    pub scope: &'static str,
    pub scanned_files: usize,
    pub scanned_bytes: u64,
    pub affected_files: usize,
    pub invocation_count: usize,
    pub mechanical_patch_count: usize,
    pub review_required_files: usize,
    pub retirement_ready: bool,
    pub verification_required: [&'static str; 4],
    pub files: Vec<TemplateMigrationFile>,
}

#[derive(Debug, Serialize)]
pub struct TemplateMigrationFile {
    pub title: String,
    pub path: String,
    pub source_sha256: String,
    pub source_bytes: usize,
    pub candidate_sha256: Option<String>,
    pub invocations: Vec<TemplateMigrationInvocation>,
    pub patches: Vec<TemplateMigrationPatch>,
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TemplateMigrationInvocation {
    pub start_byte: usize,
    pub end_byte: usize,
    pub source_sha256: String,
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TemplateMigrationPatch {
    pub start_byte: usize,
    pub end_byte: usize,
    pub before: String,
    pub after: String,
}

pub fn plan_template_migration(
    paths: &ResolvedPaths,
    spec: TemplateMigrationSpec,
) -> Result<TemplateMigrationPlan> {
    spec.validate()?;
    let scanned = scan_files(paths, &ScanOptions::default())?;
    if scanned.len() > 20_000 {
        bail!("migration workspace exceeds 20000 files; use a scoped project");
    }
    let mut plan = TemplateMigrationPlan {
        schema: "template_migration_plan_v1",
        plan_id: String::new(),
        spec,
        scope: "current_local_wikitext_files_only",
        scanned_files: 0,
        scanned_bytes: 0,
        affected_files: 0,
        invocation_count: 0,
        mechanical_patch_count: 0,
        review_required_files: 0,
        retirement_ready: false,
        verification_required: [
            "recheck_exact_source_hashes",
            "verify_live_transclusions_and_redirects",
            "compare_before_after_render_fixtures",
            "rescan_usage_before_compatibility_retirement",
        ],
        files: Vec::new(),
    };
    // All source identities contribute to the plan, including files with zero current matches.
    let mut inventory = Vec::new();
    for file in scanned {
        if !file.relative_path.ends_with(".wiki") {
            continue;
        }
        let path = paths.project_root.join(&file.relative_path);
        validate_scoped_path(paths, &path)?;
        let mut source = String::new();
        File::open(&path)?
            .take(MAX_SOURCE_BYTES + 1)
            .read_to_string(&mut source)
            .with_context(|| format!("read migration source {}", file.relative_path))?;
        if source.len() as u64 > MAX_SOURCE_BYTES {
            bail!("migration source exceeds 4 MiB: {}", file.relative_path);
        }
        plan.scanned_files += 1;
        plan.scanned_bytes += source.len() as u64;
        if plan.scanned_bytes > 256 * 1024 * 1024 {
            bail!("migration workspace exceeds 256 MiB; use a scoped project");
        }
        let result = analyze_migration_source(&source, file.title, file.relative_path, &plan.spec);
        inventory.push((
            result.title.clone(),
            result.path.clone(),
            result.source_sha256.clone(),
        ));
        if !result.invocations.is_empty() || !result.review_reasons.is_empty() {
            plan.affected_files += usize::from(!result.invocations.is_empty());
            plan.invocation_count += result.invocations.len();
            plan.mechanical_patch_count += result.patches.len();
            plan.review_required_files += usize::from(!result.review_reasons.is_empty());
            plan.files.push(result);
        }
    }
    plan.plan_id = compute_sha256(&serde_json::to_string(&(
        "template_migration_plan_v1",
        &plan.spec,
        inventory,
    ))?);
    Ok(plan)
}

fn analyze_migration_source(
    source: &str,
    title: String,
    path: String,
    spec: &TemplateMigrationSpec,
) -> TemplateMigrationFile {
    let mut result = TemplateMigrationFile {
        title,
        path,
        source_sha256: compute_sha256(source),
        source_bytes: source.len(),
        candidate_sha256: None,
        invocations: Vec::new(),
        patches: Vec::new(),
        review_reasons: Vec::new(),
    };
    let (mut ranges, unfinished) = scan_template_body_ranges(source);
    ranges.sort_by_key(|range| range.start);
    if !unfinished.is_empty() {
        result
            .review_reasons
            .push(format!("unclosed_brace_constructs:{}", unfinished.len()));
    }
    for range in ranges {
        let inner = &source[range.clone()];
        let segments = split_template_segment_ranges(inner);
        let head = inner[segments[0].clone()].trim();
        if head.contains(['{', '}', '<', '>']) {
            result
                .review_reasons
                .push(format!("dynamic_transclusion_at:{}", range.start - 2));
            continue;
        }
        if let Some((prefix, rest)) = head.split_once(':')
            && (prefix.eq_ignore_ascii_case("subst") || prefix.eq_ignore_ascii_case("safesubst"))
            && template_identity(rest, spec.title_case)
                == template_identity(&spec.from_template, spec.title_case)
        {
            result.review_reasons.push(format!(
                "substitution_requires_review_at:{}",
                range.start - 2
            ));
            continue;
        }
        if template_identity(head, spec.title_case)
            != template_identity(&spec.from_template, spec.title_case)
        {
            continue;
        }
        let mut keys = BTreeSet::new();
        let mut source_keys = BTreeSet::new();
        let mut positional = 1usize;
        let mut patches = Vec::new();
        if template_identity(&spec.from_template, spec.title_case)
            != template_identity(&spec.to_template, spec.title_case)
        {
            let replacement = canonical_template_title(&spec.to_template).expect("validated title");
            patches.push(patch(
                source,
                range.start + segments[0].start,
                &inner[segments[0].clone()],
                replacement,
            ));
        }
        for segment in segments.iter().skip(1) {
            let value = &inner[segment.clone()];
            let (key, key_text) = match split_once_top_level_equals(value) {
                Some((key, _)) => (key.trim().to_string(), Some(key)),
                None => {
                    let key = positional.to_string();
                    positional += 1;
                    (key, None)
                }
            };
            let target = spec.parameter_renames.get(&key).unwrap_or(&key);
            source_keys.insert(key.clone());
            if key.contains(['{', '}', '<', '>']) {
                result
                    .review_reasons
                    .push(format!("dynamic_parameter_at:{}", range.start - 2));
            }
            if !keys.insert(target.clone()) {
                result.review_reasons.push(format!(
                    "parameter_collision_at:{}:{target}",
                    range.start - 2
                ));
            }
            if spec.deprecated_parameters.contains(&key) {
                result
                    .review_reasons
                    .push(format!("deprecated_parameter_at:{}:{key}", range.start - 2));
            }
            if target != &key {
                match key_text {
                    Some(key_text) => patches.push(patch(
                        source,
                        range.start + segment.start,
                        &key_text,
                        target.clone(),
                    )),
                    None => result.review_reasons.push(format!(
                        "positional_rename_requires_review_at:{}:{key}",
                        range.start - 2
                    )),
                }
            }
        }
        result.invocations.push(TemplateMigrationInvocation {
            start_byte: range.start - 2,
            end_byte: range.end + 2,
            source_sha256: compute_sha256(&source[range.start - 2..range.end + 2]),
            parameter_keys: source_keys.into_iter().collect(),
        });
        result.patches.extend(patches);
    }
    result.review_reasons.sort();
    result.review_reasons.dedup();
    result.patches.sort_by_key(|patch| patch.start_byte);
    if result
        .patches
        .windows(2)
        .any(|pair| pair[0].end_byte > pair[1].start_byte)
    {
        result
            .review_reasons
            .push("overlapping_source_patches".to_string());
    }
    if result.review_reasons.is_empty() {
        let mut candidate = source.to_string();
        for patch in result.patches.iter().rev() {
            candidate.replace_range(patch.start_byte..patch.end_byte, &patch.after);
        }
        result.candidate_sha256 = Some(compute_sha256(&candidate));
    } else {
        // A file is one review unit. Never expose a partially actionable rewrite of an
        // ambiguous file while hiding its conflicting invocation elsewhere in the report.
        result.patches.clear();
    }
    result
}

fn patch(source: &str, offset: usize, text: &str, after: String) -> TemplateMigrationPatch {
    let start = offset + text.len() - text.trim_start().len();
    let end = offset + text.trim_end().len();
    TemplateMigrationPatch {
        start_byte: start,
        end_byte: end,
        before: source[start..end].to_string(),
        after,
    }
}

fn template_identity(raw: &str, title_case: MigrationTitleCase) -> Option<String> {
    let raw = raw.trim();
    // {{:Page}} transcludes the main namespace, not Template:Page.
    if raw.starts_with(':')
        && !raw
            .trim_start_matches(':')
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Template:"))
    {
        return None;
    }
    let title = canonical_template_title(raw)?;
    if matches!(title_case, MigrationTitleCase::CaseSensitive) {
        return Some(title);
    }
    let body = title.strip_prefix("Template:")?;
    let mut chars = body.chars();
    let first = chars.next()?;
    Some(format!(
        "Template:{}{}",
        first.to_uppercase(),
        chars.as_str()
    ))
}

#[cfg(test)]
mod tests;
