use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::templates::{normalize_module_lookup_title, normalize_template_lookup_title};
use crate::content_store::parsing::extract_template_invocations;
use crate::runtime::ResolvedPaths;
use crate::support::compute_sha256;
use crate::wikitext::lint::lint_wikitext;

use super::template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, find_template_catalog_entry,
};
use super::template_data::{extract_module_references, extract_source_parameters};
use super::template_engineering::transcluded_template_source;

pub const TEMPLATE_ENGINEERING_CONTRACT_SCHEMA: &str = "template_engineering_contract_v1";
const MAX_TEMPLATE_RENDER_FIXTURES: usize = 64;
const TEMPLATE_CONTRACT_ASSESSMENT_SCHEMA: &str = "template_contract_assessment_v1";
const TEMPLATE_RENDER_FIXTURE_BUNDLE_SCHEMA: &str = "template_render_fixture_bundle_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemplateEngineeringContract {
    pub schema_version: String,
    pub template_title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    pub implementation: TemplateContractImplementation,
    #[serde(default)]
    pub parameters: Vec<TemplateContractParameter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation_wikitext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation_footer_wikitext: Option<String>,
    #[serde(default)]
    pub examples: Vec<TemplateContractExample>,
    #[serde(default)]
    pub render_fixtures: Vec<TemplateRenderFixture>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemplateContractImplementation {
    pub body_wikitext: String,
    #[serde(default)]
    pub template_dependencies: Vec<String>,
    #[serde(default)]
    pub module_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemplateContractParameter {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub migrate_from: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub description: String,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub suggested: bool,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub suggested_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemplateContractExample {
    pub id: String,
    pub invocation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TemplateRenderFixture {
    pub id: String,
    pub invocation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_scope_count: Option<usize>,
    #[serde(default)]
    pub require_interactive_link: bool,
    #[serde(default)]
    pub required_href_substrings: Vec<String>,
    #[serde(default)]
    pub required_link_classes: Vec<String>,
    #[serde(default = "default_true")]
    pub forbid_literal_wikilinks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateContractAssessment {
    pub schema_version: String,
    pub status: String,
    pub compatibility: String,
    pub template_title: String,
    pub against_template_title: Option<String>,
    pub scaffold_sha256: String,
    pub scaffold_bytes: usize,
    pub parameter_changes: Vec<TemplateParameterChange>,
    pub dependency_contract: TemplateDependencyContractAssessment,
    pub render_fixture_bundle: TemplateRenderFixtureBundle,
    pub findings: Vec<TemplateContractFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateContractFinding {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateParameterChange {
    pub parameter: String,
    pub change: String,
    pub compatibility: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDependencyContractAssessment {
    pub declared_templates: Vec<String>,
    pub observed_templates: Vec<String>,
    pub declared_modules: Vec<String>,
    pub observed_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateRenderFixtureBundle {
    pub schema_version: String,
    pub template_title: String,
    pub scaffold_sha256: String,
    pub fixtures: Vec<TemplateRenderFixture>,
}

pub fn parse_template_engineering_contract(content: &str) -> Result<TemplateEngineeringContract> {
    serde_json::from_str(content).context("decode template engineering contract JSON")
}

pub fn capture_template_engineering_contract(
    paths: &ResolvedPaths,
    catalog: &TemplateCatalog,
    template_title: &str,
) -> Result<TemplateEngineeringContract> {
    let entry = match find_template_catalog_entry(catalog, template_title) {
        TemplateCatalogEntryLookup::Found(entry) => entry,
        TemplateCatalogEntryLookup::TemplateMissing { template_title } => {
            bail!("template is absent from the local catalog: {template_title}")
        }
        TemplateCatalogEntryLookup::CatalogMissing => bail!("template catalog is missing"),
    };
    let source_path = paths.project_root.join(&entry.relative_path);
    crate::filesystem::validate_scoped_path(paths, &source_path)?;
    let source = fs::read_to_string(&source_path).with_context(|| {
        format!(
            "failed to read template source {} as UTF-8",
            crate::support::normalize_path(&source_path)
        )
    })?;
    let body_wikitext = transcluded_template_source(&source).trim().to_string();
    let description = entry
        .templatedata
        .as_ref()
        .and_then(|data| data.description.clone())
        .or_else(|| entry.summary_text.clone())
        .unwrap_or_default();
    let format = entry
        .templatedata
        .as_ref()
        .and_then(|data| data.format.clone());
    let parameters = entry
        .parameters
        .iter()
        .map(|parameter| TemplateContractParameter {
            name: parameter.name.clone(),
            aliases: parameter.aliases.clone(),
            migrate_from: Vec::new(),
            label: parameter.label.clone(),
            description: parameter.description.clone().unwrap_or_default(),
            param_type: parameter.param_type.clone(),
            required: parameter.required,
            suggested: parameter.suggested,
            deprecated: parameter.deprecated,
            example: parameter.example.clone(),
            default: parameter.default_value.clone(),
            suggested_values: parameter.suggested_values.clone(),
            auto_value: parameter.auto_value.clone(),
        })
        .collect();
    let examples = entry
        .examples
        .iter()
        .take(4)
        .enumerate()
        .map(|(index, example)| TemplateContractExample {
            id: format!("captured-{}", index + 1),
            invocation: example.invocation_text.clone(),
            description: example
                .source_title
                .as_ref()
                .map(|title| format!("Observed in {title}.")),
        })
        .collect();
    let mut contract = TemplateEngineeringContract {
        schema_version: TEMPLATE_ENGINEERING_CONTRACT_SCHEMA.to_string(),
        template_title: entry.template_title.clone(),
        description,
        format,
        implementation: TemplateContractImplementation {
            body_wikitext,
            template_dependencies: Vec::new(),
            module_dependencies: Vec::new(),
        },
        parameters,
        documentation_wikitext: None,
        documentation_footer_wikitext: None,
        examples,
        render_fixtures: Vec::new(),
    };
    let mut ignored_findings = Vec::new();
    let dependencies = assess_dependency_contract(&contract, &mut ignored_findings);
    contract.implementation.template_dependencies = dependencies.observed_templates;
    contract.implementation.module_dependencies = dependencies.observed_modules;
    Ok(contract)
}

pub fn render_template_scaffold(contract: &TemplateEngineeringContract) -> Result<String> {
    let semantic_findings = validate_contract_intrinsic(contract);
    if semantic_findings
        .iter()
        .any(|finding| finding.severity == "error")
    {
        bail!(
            "template contract is invalid: {}",
            semantic_findings
                .iter()
                .filter(|finding| finding.severity == "error")
                .map(|finding| format!("{}: {}", finding.code, finding.message))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    render_template_scaffold_unchecked(contract)
}

pub fn assess_template_engineering_contract(
    contract: &TemplateEngineeringContract,
    catalog: &TemplateCatalog,
    against_template: Option<&str>,
) -> Result<TemplateContractAssessment> {
    let scaffold = render_template_scaffold_unchecked(contract)?;
    let scaffold_sha256 = compute_sha256(&scaffold);
    let mut findings = validate_contract_shape(contract);
    for issue in lint_wikitext(&scaffold) {
        findings.push(TemplateContractFinding {
            severity: "error".to_string(),
            code: issue.rule_id.to_string(),
            message: format!("{} at byte {}", issue.message, issue.byte_offset),
        });
    }

    let dependency_contract = assess_dependency_contract(contract, &mut findings);
    validate_source_parameters(contract, &mut findings);
    validate_examples(contract, &mut findings);
    validate_render_fixtures(contract, &mut findings);

    let selected_against = against_template.unwrap_or(&contract.template_title);
    let (against_template_title, mut parameter_changes, mut compatibility) =
        match find_template_catalog_entry(catalog, selected_against) {
            TemplateCatalogEntryLookup::Found(entry) => (
                Some(entry.template_title.clone()),
                compare_parameters(contract, &entry),
                "exact".to_string(),
            ),
            TemplateCatalogEntryLookup::TemplateMissing { template_title } => {
                if against_template.is_some() {
                    findings.push(TemplateContractFinding {
                        severity: "error".to_string(),
                        code: "against_template_missing".to_string(),
                        message: format!(
                            "the requested comparison target is absent from the template catalog: {template_title}"
                        ),
                    });
                }
                (None, Vec::new(), "new_template".to_string())
            }
            TemplateCatalogEntryLookup::CatalogMissing => {
                bail!("template catalog is missing");
            }
        };

    if !parameter_changes.is_empty() {
        compatibility = if parameter_changes
            .iter()
            .any(|change| change.compatibility == "breaking")
        {
            "migration_required".to_string()
        } else if parameter_changes
            .iter()
            .any(|change| change.compatibility == "review")
        {
            "review_required".to_string()
        } else {
            "compatible".to_string()
        };
    }
    parameter_changes.sort();
    findings.sort();
    findings.dedup();
    let status = if findings.iter().any(|finding| finding.severity == "error")
        || compatibility == "migration_required"
    {
        "blocked"
    } else if findings.iter().any(|finding| finding.severity == "warning")
        || compatibility == "review_required"
    {
        "review"
    } else {
        "clean"
    };

    Ok(TemplateContractAssessment {
        schema_version: TEMPLATE_CONTRACT_ASSESSMENT_SCHEMA.to_string(),
        status: status.to_string(),
        compatibility,
        template_title: normalize_template_lookup_title(&contract.template_title),
        against_template_title,
        scaffold_sha256: scaffold_sha256.clone(),
        scaffold_bytes: scaffold.len(),
        parameter_changes,
        dependency_contract,
        render_fixture_bundle: TemplateRenderFixtureBundle {
            schema_version: TEMPLATE_RENDER_FIXTURE_BUNDLE_SCHEMA.to_string(),
            template_title: normalize_template_lookup_title(&contract.template_title),
            scaffold_sha256,
            fixtures: contract.render_fixtures.clone(),
        },
        findings,
    })
}

fn render_template_scaffold_unchecked(contract: &TemplateEngineeringContract) -> Result<String> {
    let mut source = String::new();
    source.push_str("<includeonly>");
    source.push_str(contract.implementation.body_wikitext.trim());
    source.push_str("</includeonly><noinclude>\n");
    if let Some(documentation) = contract
        .documentation_wikitext
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source.push_str(documentation);
        source.push_str("\n\n");
    }
    if !contract.examples.is_empty() {
        source.push_str("== Usage ==\n");
        for example in &contract.examples {
            if let Some(description) = example
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                source.push_str(description);
                source.push('\n');
            }
            source.push_str("<pre>");
            source.push_str(&escape_preformatted_wikitext(example.invocation.trim()));
            source.push_str("</pre>\n");
        }
        source.push('\n');
    }
    source.push_str("<templatedata>\n");
    source.push_str(&render_template_data(contract)?);
    source.push_str("\n</templatedata>\n");
    if let Some(footer) = contract
        .documentation_footer_wikitext
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source.push_str(footer);
        source.push('\n');
    }
    source.push_str("</noinclude>\n");
    Ok(source)
}

fn render_template_data(contract: &TemplateEngineeringContract) -> Result<String> {
    let mut params = serde_json::Map::new();
    let mut parameter_order = Vec::new();
    for parameter in &contract.parameters {
        parameter_order.push(parameter.name.clone());
        let mut detail = serde_json::Map::new();
        insert_optional_string(&mut detail, "label", parameter.label.as_deref());
        detail.insert(
            "description".to_string(),
            serde_json::Value::String(parameter.description.clone()),
        );
        insert_optional_string(&mut detail, "type", parameter.param_type.as_deref());
        insert_optional_string(&mut detail, "example", parameter.example.as_deref());
        insert_optional_string(&mut detail, "default", parameter.default.as_deref());
        insert_optional_string(&mut detail, "autovalue", parameter.auto_value.as_deref());
        if !parameter.aliases.is_empty() {
            detail.insert(
                "aliases".to_string(),
                serde_json::to_value(&parameter.aliases)?,
            );
        }
        if !parameter.suggested_values.is_empty() {
            detail.insert(
                "suggestedvalues".to_string(),
                serde_json::to_value(&parameter.suggested_values)?,
            );
        }
        if parameter.required {
            detail.insert("required".to_string(), serde_json::Value::Bool(true));
        }
        if parameter.suggested {
            detail.insert("suggested".to_string(), serde_json::Value::Bool(true));
        }
        if parameter.deprecated {
            detail.insert("deprecated".to_string(), serde_json::Value::Bool(true));
        }
        params.insert(parameter.name.clone(), serde_json::Value::Object(detail));
    }

    let mut root = serde_json::Map::new();
    root.insert(
        "description".to_string(),
        serde_json::Value::String(contract.description.clone()),
    );
    if let Some(format) = &contract.format {
        root.insert(
            "format".to_string(),
            serde_json::Value::String(format.clone()),
        );
    }
    root.insert("params".to_string(), serde_json::Value::Object(params));
    if !parameter_order.is_empty() {
        root.insert(
            "paramOrder".to_string(),
            serde_json::to_value(parameter_order)?,
        );
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map(|encoded| {
            encoded
                .replace('&', "\\u0026")
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
        })
        .context("encode generated TemplateData")
}

fn escape_preformatted_wikitext(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn insert_optional_string(
    target: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        target.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

fn validate_contract_shape(contract: &TemplateEngineeringContract) -> Vec<TemplateContractFinding> {
    let mut findings = Vec::new();
    if contract.schema_version != TEMPLATE_ENGINEERING_CONTRACT_SCHEMA {
        error(
            &mut findings,
            "unsupported_schema",
            format!(
                "schema_version must be {TEMPLATE_ENGINEERING_CONTRACT_SCHEMA}, got {}",
                contract.schema_version
            ),
        );
    }
    let normalized_title = normalize_template_lookup_title(&contract.template_title);
    if normalized_title == "Template:" || contract.template_title.trim() != normalized_title {
        error(
            &mut findings,
            "invalid_template_title",
            "template_title must be a canonical non-empty Template: title".to_string(),
        );
    }
    if contract.description.trim().is_empty() {
        error(
            &mut findings,
            "description_missing",
            "contract description must be non-empty".to_string(),
        );
    }
    if contract
        .format
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        error(
            &mut findings,
            "format_empty",
            "format must be omitted or non-empty".to_string(),
        );
    }
    if contract.implementation.body_wikitext.trim().is_empty() {
        error(
            &mut findings,
            "implementation_body_missing",
            "implementation.body_wikitext must be non-empty".to_string(),
        );
    }
    let body_lower = contract.implementation.body_wikitext.to_ascii_lowercase();
    if body_lower.contains("<includeonly") || body_lower.contains("<noinclude") {
        error(
            &mut findings,
            "implementation_owns_wrapper",
            "implementation.body_wikitext must omit includeonly/noinclude wrappers; scaffolding owns them"
                .to_string(),
        );
    }
    for (kind, value) in [
        (
            "documentation_wikitext",
            contract.documentation_wikitext.as_deref(),
        ),
        (
            "documentation_footer_wikitext",
            contract.documentation_footer_wikitext.as_deref(),
        ),
    ]
    .into_iter()
    .chain(
        contract
            .examples
            .iter()
            .map(|example| ("example description", example.description.as_deref())),
    ) {
        if value.is_some_and(contains_template_control_tag) {
            error(
                &mut findings,
                "documentation_controls_wrapper",
                format!(
                    "{kind} must not contain includeonly, noinclude, or templatedata control tags"
                ),
            );
        }
    }

    validate_unique_ids(
        contract.examples.iter().map(|item| &item.id),
        "example",
        &mut findings,
    );
    validate_unique_ids(
        contract.render_fixtures.iter().map(|item| &item.id),
        "render fixture",
        &mut findings,
    );
    if contract.render_fixtures.len() > MAX_TEMPLATE_RENDER_FIXTURES {
        error(
            &mut findings,
            "render_fixture_limit_exceeded",
            format!("contracts may declare at most {MAX_TEMPLATE_RENDER_FIXTURES} render fixtures"),
        );
    }

    let mut parameter_names = BTreeSet::new();
    let mut accepted_names = BTreeMap::<String, String>::new();
    for parameter in &contract.parameters {
        if parameter.name.trim().is_empty() || parameter.name.trim() != parameter.name {
            error(
                &mut findings,
                "invalid_parameter_name",
                format!(
                    "parameter names must be non-empty and have no surrounding whitespace: {:?}",
                    parameter.name
                ),
            );
            continue;
        }
        if !parameter_names.insert(parameter.name.clone()) {
            error(
                &mut findings,
                "duplicate_parameter",
                format!("parameter is declared more than once: {}", parameter.name),
            );
        }
        if parameter.description.trim().is_empty() {
            error(
                &mut findings,
                "parameter_description_missing",
                format!("parameter {} must have a description", parameter.name),
            );
        }
        let mut local_names = BTreeSet::new();
        for accepted in std::iter::once(&parameter.name)
            .chain(parameter.aliases.iter())
            .chain(parameter.migrate_from.iter())
        {
            if accepted.trim().is_empty() || accepted.trim() != accepted {
                error(
                    &mut findings,
                    "invalid_parameter_alias",
                    format!(
                        "parameter {} has a blank or whitespace-padded alias/migration name",
                        parameter.name
                    ),
                );
                continue;
            }
            if !local_names.insert(accepted.clone()) {
                error(
                    &mut findings,
                    "duplicate_parameter_name",
                    format!(
                        "parameter {} repeats canonical, alias, or migrate_from name {accepted}",
                        parameter.name
                    ),
                );
            }
            if let Some(owner) = accepted_names.insert(accepted.clone(), parameter.name.clone())
                && owner != parameter.name
            {
                error(
                    &mut findings,
                    "ambiguous_parameter_name",
                    format!(
                        "parameter name or alias {accepted} belongs to both {owner} and {}",
                        parameter.name
                    ),
                );
            }
        }
    }
    findings
}

fn validate_contract_intrinsic(
    contract: &TemplateEngineeringContract,
) -> Vec<TemplateContractFinding> {
    let mut findings = validate_contract_shape(contract);
    assess_dependency_contract(contract, &mut findings);
    validate_source_parameters(contract, &mut findings);
    validate_examples(contract, &mut findings);
    validate_render_fixtures(contract, &mut findings);
    findings.sort();
    findings.dedup();
    findings
}

fn contains_template_control_tag(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "<includeonly",
        "</includeonly",
        "<onlyinclude",
        "</onlyinclude",
        "<noinclude",
        "</noinclude",
        "<templatedata",
        "</templatedata",
    ]
    .iter()
    .any(|tag| lower.contains(tag))
}

fn validate_unique_ids<'a>(
    ids: impl Iterator<Item = &'a String>,
    kind: &str,
    findings: &mut Vec<TemplateContractFinding>,
) {
    let mut seen = BTreeSet::new();
    for id in ids.cloned() {
        if id.trim().is_empty() || id.trim() != id {
            error(
                findings,
                "invalid_fixture_id",
                format!("{kind} ids must be non-empty and have no surrounding whitespace"),
            );
        } else if !seen.insert(id.clone()) {
            error(
                findings,
                "duplicate_fixture_id",
                format!("duplicate {kind} id: {id}"),
            );
        }
    }
}

fn assess_dependency_contract(
    contract: &TemplateEngineeringContract,
    findings: &mut Vec<TemplateContractFinding>,
) -> TemplateDependencyContractAssessment {
    let declared_templates = normalized_template_set(
        &contract.implementation.template_dependencies,
        &contract.template_title,
    );
    let declared_modules = normalized_module_set(&contract.implementation.module_dependencies);
    let runtime_source = transcluded_template_source(&contract.implementation.body_wikitext);
    let observed_templates = extract_template_invocations(&runtime_source)
        .into_iter()
        .map(|invocation| normalize_template_lookup_title(&invocation.template_title))
        .filter(|title| title != &normalize_template_lookup_title(&contract.template_title))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let observed_modules = extract_module_references(&runtime_source)
        .into_iter()
        .map(|title| normalize_module_lookup_title(&title))
        .filter(|title| !title.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    compare_dependency_sets(
        "template",
        &declared_templates,
        &observed_templates,
        findings,
    );
    compare_dependency_sets("module", &declared_modules, &observed_modules, findings);
    TemplateDependencyContractAssessment {
        declared_templates,
        observed_templates,
        declared_modules,
        observed_modules,
    }
}

fn normalized_template_set(values: &[String], own_title: &str) -> Vec<String> {
    let own_title = normalize_template_lookup_title(own_title);
    values
        .iter()
        .map(|value| normalize_template_lookup_title(value))
        .filter(|value| value != "Template:" && value != &own_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalized_module_set(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| normalize_module_lookup_title(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn compare_dependency_sets(
    kind: &str,
    declared: &[String],
    observed: &[String],
    findings: &mut Vec<TemplateContractFinding>,
) {
    let declared = declared.iter().collect::<BTreeSet<_>>();
    let observed = observed.iter().collect::<BTreeSet<_>>();
    for dependency in observed.difference(&declared) {
        error(
            findings,
            "dependency_not_declared",
            format!("observed {kind} dependency is not declared: {dependency}"),
        );
    }
    for dependency in declared.difference(&observed) {
        error(
            findings,
            "declared_dependency_not_observed",
            format!("declared {kind} dependency is not present in implementation: {dependency}"),
        );
    }
}

fn validate_source_parameters(
    contract: &TemplateEngineeringContract,
    findings: &mut Vec<TemplateContractFinding>,
) {
    let accepted = contract
        .parameters
        .iter()
        .flat_map(|parameter| std::iter::once(&parameter.name).chain(parameter.aliases.iter()))
        .collect::<BTreeSet<_>>();
    for observed in extract_source_parameters(&contract.implementation.body_wikitext) {
        if !accepted.contains(&observed) {
            error(
                findings,
                "implementation_parameter_unknown",
                format!(
                    "implementation reads parameter {observed}, but the contract declares no matching canonical name or alias"
                ),
            );
        }
    }
}

fn validate_examples(
    contract: &TemplateEngineeringContract,
    findings: &mut Vec<TemplateContractFinding>,
) {
    for example in &contract.examples {
        validate_invocation(
            contract,
            &example.id,
            &example.invocation,
            "example",
            findings,
        );
    }
}

fn validate_render_fixtures(
    contract: &TemplateEngineeringContract,
    findings: &mut Vec<TemplateContractFinding>,
) {
    for fixture in &contract.render_fixtures {
        validate_invocation(
            contract,
            &fixture.id,
            &fixture.invocation,
            "render fixture",
            findings,
        );
        if fixture.scope_class.is_none()
            && (fixture.expected_scope_count.is_some()
                || fixture.require_interactive_link
                || !fixture.required_href_substrings.is_empty()
                || !fixture.required_link_classes.is_empty())
        {
            error(
                findings,
                "render_scope_missing",
                format!(
                    "render fixture {} has scoped expectations but no scope_class",
                    fixture.id
                ),
            );
        }
        if fixture
            .scope_class
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.split_whitespace().count() != 1)
        {
            error(
                findings,
                "render_scope_invalid",
                format!(
                    "render fixture {} scope_class must be one non-empty CSS class",
                    fixture.id
                ),
            );
        }
    }
}

fn validate_invocation(
    contract: &TemplateEngineeringContract,
    id: &str,
    invocation: &str,
    kind: &str,
    findings: &mut Vec<TemplateContractFinding>,
) {
    let expected_title = normalize_template_lookup_title(&contract.template_title);
    let Some(parsed) = extract_template_invocations(invocation)
        .into_iter()
        .find(|item| normalize_template_lookup_title(&item.template_title) == expected_title)
    else {
        error(
            findings,
            "fixture_template_missing",
            format!("{kind} {id} does not invoke {expected_title}"),
        );
        return;
    };
    let accepted = contract
        .parameters
        .iter()
        .flat_map(|parameter| std::iter::once(&parameter.name).chain(parameter.aliases.iter()))
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in parsed.parameter_keys {
        let normalized = key.strip_prefix('$').unwrap_or(&key);
        if !accepted.contains(normalized) {
            error(
                findings,
                "fixture_parameter_unknown",
                format!("{kind} {id} uses undeclared parameter {normalized}"),
            );
        }
    }
}

fn compare_parameters(
    contract: &TemplateEngineeringContract,
    existing: &TemplateCatalogEntry,
) -> Vec<TemplateParameterChange> {
    let mut changes = Vec::new();
    let existing_by_name = existing
        .parameters
        .iter()
        .flat_map(|parameter| {
            std::iter::once(parameter.name.as_str())
                .chain(parameter.aliases.iter().map(String::as_str))
                .chain(parameter.observed_names.iter().map(String::as_str))
                .map(move |name| (name.to_string(), parameter))
        })
        .collect::<BTreeMap<_, _>>();
    let mut claimed_existing = BTreeSet::new();

    for desired in &contract.parameters {
        let exact = std::iter::once(&desired.name)
            .chain(desired.aliases.iter())
            .find_map(|name| existing_by_name.get(name).copied());
        let migrated = desired
            .migrate_from
            .iter()
            .find_map(|name| existing_by_name.get(name).copied());
        let Some(current) = exact.or(migrated) else {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: if desired.required {
                    "required_parameter_added"
                } else {
                    "optional_parameter_added"
                }
                .to_string(),
                compatibility: if desired.required {
                    "breaking"
                } else {
                    "compatible"
                }
                .to_string(),
                detail: "parameter is absent from the compared catalog contract".to_string(),
            });
            continue;
        };
        if !claimed_existing.insert(current.name.clone()) {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "existing_parameter_claimed_multiple_times".to_string(),
                compatibility: "breaking".to_string(),
                detail: format!(
                    "more than one desired parameter resolves to existing parameter {}",
                    current.name
                ),
            });
        }
        let preserved_names = std::iter::once(&desired.name)
            .chain(desired.aliases.iter())
            .chain(desired.migrate_from.iter())
            .collect::<BTreeSet<_>>();
        for old_name in current
            .aliases
            .iter()
            .chain(current.observed_names.iter())
            .filter(|name| *name != &current.name)
            .collect::<BTreeSet<_>>()
        {
            if !preserved_names.contains(old_name) {
                changes.push(TemplateParameterChange {
                    parameter: desired.name.clone(),
                    change: "alias_removed_without_mapping".to_string(),
                    compatibility: "breaking".to_string(),
                    detail: format!(
                        "existing accepted parameter name {old_name} is absent from aliases and migrate_from"
                    ),
                });
            }
        }
        if migrated.is_some() && exact.is_none() {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "parameter_renamed".to_string(),
                compatibility: "review".to_string(),
                detail: format!(
                    "explicit migrate_from maps existing parameter {} to {}",
                    current.name, desired.name
                ),
            });
        }
        if !current.required && desired.required {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "requiredness_strengthened".to_string(),
                compatibility: "breaking".to_string(),
                detail: "an optional parameter becomes required".to_string(),
            });
        } else if current.required && !desired.required {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "requiredness_relaxed".to_string(),
                compatibility: "compatible".to_string(),
                detail: "a required parameter becomes optional".to_string(),
            });
        }
        if current.param_type != desired.param_type {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "type_changed".to_string(),
                compatibility: "review".to_string(),
                detail: format!(
                    "TemplateData type changes from {} to {}",
                    current.param_type.as_deref().unwrap_or("<none>"),
                    desired.param_type.as_deref().unwrap_or("<none>")
                ),
            });
        }
        if !current.deprecated && desired.deprecated {
            changes.push(TemplateParameterChange {
                parameter: desired.name.clone(),
                change: "parameter_deprecated".to_string(),
                compatibility: "review".to_string(),
                detail: "parameter becomes deprecated".to_string(),
            });
        }
    }

    for current in &existing.parameters {
        if !claimed_existing.contains(&current.name) {
            changes.push(TemplateParameterChange {
                parameter: current.name.clone(),
                change: "parameter_removed_without_mapping".to_string(),
                compatibility: "breaking".to_string(),
                detail: format!(
                    "existing parameter has no canonical, alias, or migrate_from match; observed usage count {}",
                    current.usage_count
                ),
            });
        }
    }
    changes
}

fn error(findings: &mut Vec<TemplateContractFinding>, code: &str, message: String) {
    findings.push(TemplateContractFinding {
        severity: "error".to_string(),
        code: code.to_string(),
        message,
    });
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::TemplateCatalogParameter;

    fn contract() -> TemplateEngineeringContract {
        TemplateEngineeringContract {
            schema_version: TEMPLATE_ENGINEERING_CONTRACT_SCHEMA.to_string(),
            template_title: "Template:Card".to_string(),
            description: "Displays a compact card.".to_string(),
            format: Some("block".to_string()),
            implementation: TemplateContractImplementation {
                body_wikitext:
                    "<templatestyles src=\"Module:Card/styles.css\" />{{Helper|text={{{text|}}}}}"
                        .to_string(),
                template_dependencies: vec!["Template:Helper".to_string()],
                module_dependencies: vec!["Module:Card/styles.css".to_string()],
            },
            parameters: vec![TemplateContractParameter {
                name: "text".to_string(),
                aliases: vec!["body".to_string()],
                migrate_from: vec!["content".to_string()],
                label: Some("Text".to_string()),
                description: "Card body.".to_string(),
                param_type: Some("content".to_string()),
                required: true,
                suggested: false,
                deprecated: false,
                example: Some("Example".to_string()),
                default: None,
                suggested_values: Vec::new(),
                auto_value: None,
            }],
            documentation_wikitext: Some("Displays a compact card.".to_string()),
            documentation_footer_wikitext: Some("[[Category:Card templates]]".to_string()),
            examples: vec![TemplateContractExample {
                id: "basic".to_string(),
                invocation: "{{Card|text=Example}}".to_string(),
                description: None,
            }],
            render_fixtures: vec![TemplateRenderFixture {
                id: "basic".to_string(),
                invocation: "{{Card|text=Example}}".to_string(),
                scope_class: Some("card".to_string()),
                expected_scope_count: Some(1),
                require_interactive_link: false,
                required_href_substrings: Vec::new(),
                required_link_classes: Vec::new(),
                forbid_literal_wikilinks: true,
            }],
        }
    }

    fn catalog(existing_name: &str) -> TemplateCatalog {
        TemplateCatalog {
            schema_version: "template_catalog_v4".to_string(),
            site_adapter_id: "test".to_string(),
            refreshed_at: "1".to_string(),
            template_count: 1,
            templatedata_count: 1,
            redirect_alias_count: 0,
            usage_index_ready: true,
            entries: vec![TemplateCatalogEntry {
                template_title: "Template:Card".to_string(),
                relative_path: "templates/Template_Card.wiki".to_string(),
                category: "test".to_string(),
                summary_text: Some("Card".to_string()),
                templatedata: None,
                redirect_aliases: Vec::new(),
                usage_aliases: Vec::new(),
                usage_count: 1,
                distinct_page_count: 1,
                example_pages: Vec::new(),
                documentation_titles: Vec::new(),
                implementation_titles: Vec::new(),
                implementation_preview: None,
                module_titles: Vec::new(),
                declared_parameter_keys: vec![existing_name.to_string()],
                parameters: vec![TemplateCatalogParameter {
                    name: existing_name.to_string(),
                    aliases: Vec::new(),
                    observed_names: vec![existing_name.to_string()],
                    sources: vec!["templatedata".to_string()],
                    label: None,
                    description: None,
                    param_type: Some("content".to_string()),
                    required: true,
                    suggested: false,
                    deprecated: false,
                    usage_count: 3,
                    example_values: Vec::new(),
                    example: None,
                    default_value: None,
                    suggested_values: Vec::new(),
                    auto_value: None,
                }],
                examples: Vec::new(),
                recommendation_tags: Vec::new(),
            }],
        }
    }

    #[test]
    fn scaffold_is_deterministic_and_contains_templatedata_and_usage() {
        let contract = contract();
        let first = render_template_scaffold(&contract).expect("scaffold");
        let second = render_template_scaffold(&contract).expect("scaffold again");
        assert_eq!(first, second);
        assert!(first.starts_with("<includeonly>"));
        assert!(first.contains("<templatedata>"));
        assert!(first.contains("\"paramOrder\""));
        assert!(first.contains("<pre>{{Card|text=Example}}</pre>"));
        assert!(first.ends_with("</templatedata>\n[[Category:Card templates]]\n</noinclude>\n"));
    }

    #[test]
    fn scaffold_rejects_unbounded_render_fixture_batches() {
        let mut contract = contract();
        contract.render_fixtures = vec![contract.render_fixtures[0].clone(); 65];
        for (index, fixture) in contract.render_fixtures.iter_mut().enumerate() {
            fixture.id = format!("fixture-{index}");
        }
        assert!(render_template_scaffold(&contract).is_err());
    }

    #[test]
    fn assessment_reports_explicit_rename_without_auto_migration() {
        let assessment =
            assess_template_engineering_contract(&contract(), &catalog("content"), None)
                .expect("assessment");
        assert_eq!(assessment.status, "review");
        assert_eq!(assessment.compatibility, "review_required");
        assert!(
            assessment
                .parameter_changes
                .iter()
                .any(|change| change.change == "parameter_renamed")
        );
        assert!(assessment.findings.is_empty());
        assert_eq!(assessment.render_fixture_bundle.fixtures.len(), 1);
    }

    #[test]
    fn assessment_blocks_undeclared_dependencies_and_unmapped_removal() {
        let mut contract = contract();
        contract.implementation.template_dependencies.clear();
        contract.parameters[0].migrate_from.clear();
        let assessment = assess_template_engineering_contract(&contract, &catalog("content"), None)
            .expect("assessment");
        assert_eq!(assessment.status, "blocked");
        assert_eq!(assessment.compatibility, "migration_required");
        assert!(
            assessment
                .findings
                .iter()
                .any(|finding| finding.code == "dependency_not_declared")
        );
        assert!(
            assessment
                .parameter_changes
                .iter()
                .any(|change| change.change == "parameter_removed_without_mapping")
        );
    }
}
