use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::artifact::{
    atomic_write, atomic_write_json, portable, relative_locator, resolve_existing_plain_file,
    resolve_output_path, sha256_bytes, sha256_file, unix_ms,
};
use crate::canonical::canonicalize_exact_paths;
use crate::catalog::{Manifest, load_manifest, resolve_manifest};
use crate::identity::{
    current_driver_identity, repository_binary_locator, verify_path_identity,
    verify_recorded_identity,
};
use crate::model::{ArtifactIdentity, OutputArtifact, ToolIdentity};
use crate::process::{CapturedStream, probe_version, run_bounded};
use crate::prose_model::{
    AUTHOR_REQUEST_SCHEMA, AUTHOR_SUBMISSION_SCHEMA, AuthorRequest, AuthorStageReceipt,
    AuthorSubmission, AxisVerdict, BoundInput, CLAIM_MAP_SCHEMA, ClaimMap, MechanicalObservation,
    OracleEvaluation, PROSE_PACKET_SCHEMA, PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA,
    PacketBinding, PacketInput, PacketSource, ParticipantExport, ProseAssignment,
    ProseAssignmentIdentity, ProseCoverageStatus, ProseMode, ProseReceipt, ProseRunStatus,
    ProseSuite, ProseSuiteIdentity, ProseSuiteReceipt, ProseSuiteRun, ProseSuiteStatus,
    REVIEW_REQUEST_SCHEMA, REVIEW_SUBMISSION_SCHEMA, ReviewAssignmentProjection, ReviewDisposition,
    ReviewPacketBinding, ReviewRequest, ReviewScope, ReviewStageReceipt, ReviewSubmission,
    SourceInput, SourceVerdict,
};
use crate::runner::create_execution_workspace;

const PROSE_PROCESS_TIMEOUT: Duration = Duration::from_secs(120);
const PARTICIPANT_EXPORT_ROOT_PREFIX: &str = "wikitest:participant-export:";
const PROSE_SUITE_CATALOG_PREFIX: &str = "wikitest:catalog:prose-suite:";
const MECHANICAL_ROOT_TOKEN: &str = "<WIKITEST_MECHANICAL_ROOT>";
const MECHANICAL_DATA_TOKEN: &str = "<WIKITEST_MECHANICAL_DATA>";
const MECHANICAL_CONFIG_TOKEN: &str = "<WIKITEST_MECHANICAL_CONFIG>";
const EXECUTION_WORKSPACES_TOKEN: &str = "<WIKITEST_EXECUTION_WORKSPACES>";
const TOOL_BINARY_TOKEN: &str = "<WIKITEST_TOOL_BINARY>";
static PROSE_RUN_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct ProseOptions {
    pub repository: PathBuf,
    pub artifacts_root: PathBuf,
    pub wikitool: PathBuf,
    pub catalogs: Vec<PathBuf>,
    pub maximum_output_bytes: usize,
}

#[derive(Debug)]
pub struct PreparedProse {
    pub receipt: ProseReceipt,
    pub receipt_path: PathBuf,
}

#[derive(Debug)]
pub struct PreparedProseSuite {
    pub receipt: ProseSuiteReceipt,
    pub receipt_path: PathBuf,
}

struct ReviewRequestParts {
    instructions: Vec<PacketInput>,
    context: Vec<PacketInput>,
    sources: Vec<PacketSource>,
    candidate: Option<ArtifactIdentity>,
    mechanical_observations: Vec<MechanicalObservation>,
}

struct ReviewRequestArtifacts {
    packet: ArtifactIdentity,
    request: ArtifactIdentity,
    export: ParticipantExport,
}

pub fn prepare_assignment(path: &Path, options: &ProseOptions) -> Result<PreparedProse> {
    let path = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve prose assignment {}", path.display()))?;
    let (manifest, assignment_bytes) = load_manifest(&path)?;
    let Manifest::ProseAssignment(assignment) = manifest else {
        bail!("{} is not a prose assignment", path.display());
    };
    let assignment = *assignment;
    let assignment_directory = path.parent().context("assignment path has no parent")?;
    let created_at = unix_ms()?;
    let nonce = PROSE_RUN_NONCE.fetch_add(1, Ordering::Relaxed);
    let run_id = format!("prose-run-{created_at}-{}-{nonce}", std::process::id(),);
    let participant_assignment_id =
        format!("case-{created_at:x}-{:x}-{nonce:x}", std::process::id());
    let run_directory = options.artifacts_root.join(&run_id);
    if run_directory.exists() {
        bail!("prose run already exists: {}", run_directory.display());
    }
    fs::create_dir_all(run_directory.join("inputs"))?;
    fs::create_dir_all(run_directory.join("holdout"))?;
    fs::create_dir_all(run_directory.join("tool"))?;

    let authority_path = run_directory.join("holdout/assignment.json");
    atomic_write(&authority_path, &assignment_bytes)?;
    let authority = artifact_identity(&run_directory, &authority_path)?;
    let public_assignment_bytes = oracle_redacted_assignment_bytes(&assignment)?;
    let assignment_path = run_directory.join("inputs/assignment.json");
    atomic_write(&assignment_path, &public_assignment_bytes)?;
    let assignment_artifact = artifact_identity(&run_directory, &assignment_path)?;
    let public_assignment_identity = ProseAssignmentIdentity {
        id: assignment.id.clone(),
        title: assignment.title.clone(),
        mode: assignment.mode,
        coverage: assignment.coverage.clone(),
        locator: assignment_artifact.locator.clone(),
        sha256: assignment_artifact.sha256.clone(),
    };

    let mut inputs = vec![assignment_artifact];
    let author_instructions = snapshot_bound_inputs(
        &assignment.author_instructions,
        assignment_directory,
        &options.repository,
        &run_directory,
        "inputs/author-instructions",
        &mut inputs,
    )?;
    let review_instructions = snapshot_bound_inputs(
        &assignment.review_instructions,
        assignment_directory,
        &options.repository,
        &run_directory,
        "inputs/review-instructions",
        &mut inputs,
    )?;
    let context = snapshot_bound_inputs(
        &assignment.context,
        assignment_directory,
        &options.repository,
        &run_directory,
        "inputs/context",
        &mut inputs,
    )?;
    let sources = snapshot_sources(
        &assignment.sources,
        assignment_directory,
        &run_directory,
        &mut inputs,
    )?;
    let fixture_candidate = assignment
        .candidate
        .as_ref()
        .map(|input| {
            snapshot_single_input(
                input,
                assignment_directory,
                &run_directory,
                "inputs/candidate",
            )
        })
        .transpose()?;
    if let Some(candidate) = &fixture_candidate {
        inputs.push(candidate.clone());
    }
    let fixture_claim_map = assignment
        .claim_map
        .as_ref()
        .map(|input| {
            snapshot_single_input(
                input,
                assignment_directory,
                &run_directory,
                "inputs/claim-map",
            )
        })
        .transpose()?;
    if let Some(claim_map) = &fixture_claim_map {
        inputs.push(claim_map.clone());
    }
    let author_templates = if assignment.mode == ProseMode::Authoring {
        let templates = write_author_templates(
            &run_id,
            &participant_assignment_id,
            &assignment,
            &run_directory,
        )?;
        inputs.push(templates.0.clone());
        inputs.push(templates.1.clone());
        Some(templates)
    } else {
        None
    };

    let packet_binding = PacketBinding {
        schema: PROSE_PACKET_SCHEMA.to_owned(),
        assignment: public_assignment_identity.clone(),
        inputs: inputs.clone(),
    };
    let packet_path = run_directory.join("inputs/packet.json");
    atomic_write_json(&packet_path, &packet_binding)?;
    let packet = artifact_identity(&run_directory, &packet_path)?;
    let tool = tool_identity(options, &run_directory)?;

    let mut author_request = None;
    let mut author_export = None;
    let mut review_request = None;
    let mut review_export = None;
    let mut review_packet = None;
    let status = match assignment.mode {
        ProseMode::Authoring => {
            let (submission_template, claim_map_template) = author_templates
                .context("authoring assignment has no generated submission templates")?;
            let request = AuthorRequest {
                schema: AUTHOR_REQUEST_SCHEMA.to_owned(),
                run_id: run_id.clone(),
                assignment_id: participant_assignment_id.clone(),
                packet_sha256: packet.sha256.clone(),
                artifact_root: "..".to_owned(),
                article: assignment.article.clone(),
                instructions: author_instructions,
                context: context.clone(),
                sources: sources.clone(),
                do_not_assert: assignment.do_not_assert.clone(),
                allowed_decisions: assignment.allowed_decisions.clone(),
                candidate_format: "MediaWiki wikitext encoded as UTF-8".to_owned(),
                claim_map_schema: CLAIM_MAP_SCHEMA.to_owned(),
                submission_schema: AUTHOR_SUBMISSION_SCHEMA.to_owned(),
                submission_template,
                claim_map_template,
            };
            let request_path = run_directory.join("author/request.json");
            atomic_write_json(&request_path, &request)?;
            author_request = Some(artifact_identity(&run_directory, &request_path)?);
            let visible_inputs = inputs
                .iter()
                .filter(|artifact| author_visible_locator(&artifact.locator))
                .cloned()
                .collect::<Vec<_>>();
            author_export = Some(write_participant_export(
                &run_id,
                "author",
                &options.repository,
                &run_directory,
                &request_path,
                &visible_inputs,
                &[],
            )?);
            ProseRunStatus::AwaitingAuthor
        }
        ProseMode::Review => {
            let candidate = fixture_candidate
                .as_ref()
                .context("validated review assignment has no candidate")?;
            let mechanical = run_mechanical_observations(
                candidate,
                &assignment.article.title,
                &run_directory,
                options,
            )?;
            let review_inputs = review_visible_inputs(&inputs, Some(candidate));
            let generated = build_review_request(
                &run_id,
                &participant_assignment_id,
                &assignment,
                &review_inputs,
                ReviewRequestParts {
                    instructions: review_instructions,
                    context,
                    sources,
                    candidate: Some(candidate.clone()),
                    mechanical_observations: mechanical,
                },
                &run_directory,
                &options.repository,
            )?;
            review_packet = Some(generated.packet);
            review_request = Some(generated.request);
            review_export = Some(generated.export);
            ProseRunStatus::AwaitingReview
        }
    };

    let receipt = ProseReceipt {
        schema: PROSE_RECEIPT_SCHEMA.to_owned(),
        run_id,
        participant_assignment_id,
        driver: current_driver_identity(&options.repository)?,
        public_assignment: public_assignment_identity,
        tool,
        status,
        created_at_unix_ms: created_at,
        updated_at_unix_ms: created_at,
        evaluation_complete: false,
        authority,
        packet,
        inputs,
        author_request,
        author_export,
        author: None,
        review_request,
        review_export,
        review_packet,
        review: None,
    };
    let receipt_path = run_directory.join("receipt.json");
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProse {
        receipt,
        receipt_path,
    })
}

pub fn prepare_suite(path: &Path, options: &ProseOptions) -> Result<PreparedProseSuite> {
    let path = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve prose suite {}", path.display()))?;
    let (manifest, suite_bytes) = load_manifest(&path)?;
    let Manifest::ProseSuite(suite) = manifest else {
        bail!("{} is not a prose suite", path.display());
    };
    let prepared_coverage = prose_suite_coverage(&suite, &options.catalogs)?;
    let required = suite
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if !required.is_subset(&prepared_coverage) {
        let missing = required
            .difference(&prepared_coverage)
            .cloned()
            .collect::<Vec<_>>();
        bail!(
            "prose suite '{}' is missing coverage {:?}",
            suite.id,
            missing
        );
    }

    let created_at = unix_ms()?;
    let run_id = format!(
        "prose-suite-{}-{created_at}-{}",
        suite.id,
        std::process::id()
    );
    let run_directory = options.artifacts_root.join(&run_id);
    fs::create_dir_all(run_directory.join("inputs"))?;
    fs::create_dir_all(run_directory.join("prepared"))?;
    fs::create_dir_all(run_directory.join("runs"))?;
    let suite_input = run_directory.join("inputs/prose-suite.json");
    atomic_write(&suite_input, &suite_bytes)?;
    let (suite_sha256, _) = sha256_file(&suite_input)?;
    let suite_identity = ProseSuiteIdentity {
        id: suite.id.clone(),
        title: suite.title.clone(),
        locator: prose_suite_catalog_locator(&suite.id),
        sha256: suite_sha256,
    };

    let mut runs = Vec::new();
    let mut child_options = options.clone();
    child_options.artifacts_root = run_directory.join("runs");
    for assignment_id in &suite.assignments {
        let assignment_path =
            resolve_manifest(assignment_id, &options.catalogs, "prose_assignment")?;
        let prepared = prepare_assignment(&assignment_path, &child_options)?;
        let snapshot_path = run_directory
            .join("prepared")
            .join(format!("{assignment_id}.receipt.json"));
        let bytes = fs::read(&prepared.receipt_path)?;
        atomic_write(&snapshot_path, &bytes)?;
        let child_directory = prepared
            .receipt_path
            .parent()
            .context("prepared prose receipt has no parent")?;
        runs.push(ProseSuiteRun {
            assignment_id: assignment_id.clone(),
            run_locator: relative_locator(&run_directory, child_directory)?,
            preparation_receipt: artifact_identity(&run_directory, &snapshot_path)?,
            current_receipt: artifact_identity(&run_directory, &prepared.receipt_path)?,
            status: prepared.receipt.status,
            coverage_status: ProseCoverageStatus::AwaitingReview,
        });
    }
    let receipt = ProseSuiteReceipt {
        schema: PROSE_SUITE_RECEIPT_SCHEMA.to_owned(),
        run_id,
        driver: current_driver_identity(&options.repository)?,
        suite: suite_identity,
        created_at_unix_ms: created_at,
        updated_at_unix_ms: created_at,
        status: ProseSuiteStatus::Prepared,
        evaluation_complete: false,
        required_coverage: suite.required_coverage,
        prepared_coverage: prepared_coverage.into_iter().collect(),
        demonstrated_coverage: Vec::new(),
        runs,
    };
    let receipt_path = run_directory.join("receipt.json");
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProseSuite {
        receipt,
        receipt_path,
    })
}

pub fn evaluate_suite(run: &Path, options: &ProseOptions) -> Result<PreparedProseSuite> {
    let receipt_candidate = if run.is_dir() {
        run.join("receipt.json")
    } else {
        run.to_path_buf()
    };
    let receipt_path = fs::canonicalize(&receipt_candidate)
        .with_context(|| format!("failed to resolve prose suite run {}", run.display()))?;
    let mut receipt: ProseSuiteReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("invalid strict prose suite receipt JSON")?;
    if receipt.schema != PROSE_SUITE_RECEIPT_SCHEMA {
        bail!(
            "unsupported prose suite receipt schema '{}'",
            receipt.schema
        );
    }
    verify_recorded_identity(&options.repository, &receipt.driver, "Wikitest driver")?;
    let suite_root = receipt_path
        .parent()
        .context("prose suite receipt has no parent")?;

    let mut demonstrated = BTreeSet::new();
    let mut evaluation_complete = true;
    let mut demonstration_failed = false;
    for run in &mut receipt.runs {
        let live_receipt_path = resolve_prose_suite_child_receipt(suite_root, run)?;
        let (live, _, live_directory, _public_assignment) = load_prose_run(&live_receipt_path)?;
        verify_current_receipt(&options.repository, &live_directory, &live)?;
        let assignment = load_authority_assignment(&live, &live_directory)?;
        if live.public_assignment.id != run.assignment_id {
            bail!(
                "prose suite run '{}' resolved to assignment '{}'",
                run.assignment_id,
                live.public_assignment.id
            );
        }
        run.status = live.status;
        let reviewed = live.evaluation_complete && live.review.is_some();
        evaluation_complete &= reviewed;
        if reviewed {
            run.coverage_status = prose_coverage_status(&live, &assignment, &live_directory)?;
            demonstration_failed |= run.coverage_status != ProseCoverageStatus::Demonstrated;
            if run.coverage_status == ProseCoverageStatus::Demonstrated {
                demonstrated.extend(assignment.coverage);
            }
        } else {
            run.coverage_status = ProseCoverageStatus::AwaitingReview;
        }
        run.current_receipt = artifact_identity(suite_root, &live_receipt_path)?;
    }
    let required = receipt
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    receipt.demonstrated_coverage = demonstrated.iter().cloned().collect();
    receipt.evaluation_complete = evaluation_complete;
    receipt.status = if !evaluation_complete {
        ProseSuiteStatus::Prepared
    } else if demonstration_failed || !required.is_subset(&demonstrated) {
        ProseSuiteStatus::Failed
    } else {
        ProseSuiteStatus::Passed
    };
    receipt.updated_at_unix_ms = unix_ms()?;
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProseSuite {
        receipt,
        receipt_path,
    })
}

pub(crate) fn resolve_prose_suite_child_receipt(
    suite_root: &Path,
    run: &ProseSuiteRun,
) -> Result<PathBuf> {
    let child_directory = resolve_output_path(suite_root, &run.run_locator)?;
    let expected = child_directory.join("receipt.json");
    let recorded = resolve_output_path(suite_root, &run.current_receipt.locator)?;
    if recorded != expected {
        bail!(
            "prose suite child '{}' receipt locator is not rooted under its run locator",
            run.assignment_id
        );
    }
    Ok(expected)
}

pub(crate) fn prose_suite_catalog_locator(suite_id: &str) -> String {
    format!("{PROSE_SUITE_CATALOG_PREFIX}{suite_id}")
}

pub fn submit_author(
    run: &Path,
    submission_path: &Path,
    options: &ProseOptions,
) -> Result<PreparedProse> {
    let (mut receipt, receipt_path, run_directory, assignment) = load_prose_run(run)?;
    verify_current_receipt(&options.repository, &run_directory, &receipt)?;
    verify_path_identity(&options.wikitool, &receipt.tool, "configured Wikitool")?;
    if assignment.mode != ProseMode::Authoring
        || receipt.status != ProseRunStatus::AwaitingAuthor
        || receipt.author.is_some()
    {
        bail!("prose run is not awaiting an author submission");
    }
    let submission_path = fs::canonicalize(submission_path).with_context(|| {
        format!(
            "failed to resolve author submission {}",
            submission_path.display()
        )
    })?;
    let submission_bytes = fs::read(&submission_path)?;
    let submission: AuthorSubmission = serde_json::from_slice(&submission_bytes)
        .context("invalid strict author submission JSON")?;
    submission.validate()?;
    if submission.run_id != receipt.run_id
        || submission.assignment_id != receipt.participant_assignment_id
        || submission.packet_sha256 != receipt.packet.sha256
    {
        bail!("author submission does not match the prepared run and packet");
    }
    if !assignment.allowed_decisions.contains(&submission.decision) {
        bail!(
            "author decision {:?} is not allowed by assignment",
            submission.decision
        );
    }
    let submission_directory = submission_path
        .parent()
        .context("author submission path has no parent")?;
    let candidate = submission
        .candidate
        .as_ref()
        .map(|reference| {
            snapshot_submission_file(
                reference,
                submission_directory,
                &run_directory,
                "author/candidate.wiki",
            )
        })
        .transpose()?;
    let claim_map_artifact = submission
        .claim_map
        .as_ref()
        .map(|reference| {
            snapshot_submission_file(
                reference,
                submission_directory,
                &run_directory,
                "author/claim-map.json",
            )
        })
        .transpose()?;
    if let (Some(candidate), Some(claim_map_artifact)) = (&candidate, &claim_map_artifact) {
        let claim_map_path = resolve_output_path(&run_directory, &claim_map_artifact.locator)?;
        let claim_map: ClaimMap = serde_json::from_slice(&fs::read(claim_map_path)?)
            .context("invalid strict claim map JSON")?;
        claim_map.validate(&assignment, &candidate.sha256)?;
    }

    let retained_submission_path = run_directory.join("author/submission.json");
    atomic_write(&retained_submission_path, &submission_bytes)?;
    let retained_submission = artifact_identity(&run_directory, &retained_submission_path)?;
    let mechanical = match &candidate {
        Some(candidate) => run_mechanical_observations(
            candidate,
            &assignment.article.title,
            &run_directory,
            options,
        )?,
        None => Vec::new(),
    };
    let review_instructions = packet_inputs_for_prefix(
        &receipt.inputs,
        "inputs/review-instructions/",
        &assignment.review_instructions,
    )?;
    let context =
        packet_inputs_for_prefix(&receipt.inputs, "inputs/context/", &assignment.context)?;
    let sources = packet_sources_from_receipt(&receipt.inputs, &assignment.sources)?;
    let review_inputs = review_visible_inputs(&receipt.inputs, candidate.as_ref());
    let generated = build_review_request(
        &receipt.run_id,
        &receipt.participant_assignment_id,
        &assignment,
        &review_inputs,
        ReviewRequestParts {
            instructions: review_instructions,
            context,
            sources,
            candidate: candidate.clone(),
            mechanical_observations: mechanical.clone(),
        },
        &run_directory,
        &options.repository,
    )?;
    receipt.author = Some(AuthorStageReceipt {
        submitted_at_unix_ms: unix_ms()?,
        author: submission.author,
        decision: submission.decision,
        submission: retained_submission,
        candidate,
        claim_map: claim_map_artifact,
        notes: submission.notes,
        holds: submission.holds,
        mechanical_observations: mechanical,
    });
    receipt.review_packet = Some(generated.packet);
    receipt.review_request = Some(generated.request);
    receipt.review_export = Some(generated.export);
    receipt.status = ProseRunStatus::AwaitingReview;
    receipt.updated_at_unix_ms = unix_ms()?;
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProse {
        receipt,
        receipt_path,
    })
}

pub fn submit_review(
    run: &Path,
    submission_path: &Path,
    options: &ProseOptions,
) -> Result<PreparedProse> {
    let (mut receipt, receipt_path, run_directory, _public_assignment) = load_prose_run(run)?;
    let assignment = load_authority_assignment(&receipt, &run_directory)?;
    verify_current_receipt(&options.repository, &run_directory, &receipt)?;
    verify_path_identity(&options.wikitool, &receipt.tool, "configured Wikitool")?;
    if receipt.status != ProseRunStatus::AwaitingReview || receipt.review.is_some() {
        bail!("prose run is not awaiting a review submission");
    }
    let review_packet = receipt
        .review_packet
        .as_ref()
        .context("prose run has no review packet")?;
    let submission_path = fs::canonicalize(submission_path).with_context(|| {
        format!(
            "failed to resolve review submission {}",
            submission_path.display()
        )
    })?;
    let submission_bytes = fs::read(&submission_path)?;
    let submission: ReviewSubmission = serde_json::from_slice(&submission_bytes)
        .context("invalid strict review submission JSON")?;
    submission.validate(&assignment)?;
    if submission.run_id != receipt.run_id
        || submission.assignment_id != receipt.participant_assignment_id
        || submission.review_packet_sha256 != review_packet.sha256
    {
        bail!("review submission does not match the prepared review packet");
    }
    let expected_candidate_sha256 = receipt
        .author
        .as_ref()
        .and_then(|author| author.candidate.as_ref())
        .or_else(|| {
            receipt.inputs.iter().find(|artifact| {
                artifact.locator.starts_with("inputs/candidate/")
                    || artifact.locator == "inputs/candidate"
            })
        })
        .map(|artifact| artifact.sha256.as_str());
    if submission.candidate_sha256.as_deref() != expected_candidate_sha256 {
        bail!("review submission candidate hash does not match the frozen candidate");
    }
    ensure_independent_reviewer(
        receipt.author.as_ref().map(|author| &author.author),
        &submission.reviewer,
    )?;

    let retained_submission_path = run_directory.join("review/submission.json");
    atomic_write(&retained_submission_path, &submission_bytes)?;
    let retained_submission = artifact_identity(&run_directory, &retained_submission_path)?;
    let oracle = assignment
        .oracle
        .as_ref()
        .map(|oracle| evaluate_oracle(oracle, &submission));
    receipt.review = Some(ReviewStageReceipt {
        submitted_at_unix_ms: unix_ms()?,
        reviewer: submission.reviewer,
        submission: retained_submission,
        scope: submission.scope,
        reader_verdict: submission.reader_verdict,
        source_verdict: submission.source_verdict,
        disposition: submission.disposition,
        finding_count: submission.findings.len(),
        residual_risk: submission.residual_risk,
        oracle,
    });
    receipt.status = match submission.disposition {
        ReviewDisposition::Accept => ProseRunStatus::ReviewedAccept,
        ReviewDisposition::Revise => ProseRunStatus::ReviewedRevise,
        ReviewDisposition::Block => ProseRunStatus::ReviewedBlock,
    };
    receipt.evaluation_complete = true;
    receipt.updated_at_unix_ms = unix_ms()?;
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProse {
        receipt,
        receipt_path,
    })
}

pub fn prose_suite_coverage(suite: &ProseSuite, catalogs: &[PathBuf]) -> Result<BTreeSet<String>> {
    let mut coverage = BTreeSet::new();
    for assignment_id in &suite.assignments {
        let path = resolve_manifest(assignment_id, catalogs, "prose_assignment")?;
        let (manifest, _) = load_manifest(&path)?;
        let Manifest::ProseAssignment(assignment) = manifest else {
            bail!("prose suite entry '{assignment_id}' is not a prose assignment");
        };
        let assignment = *assignment;
        coverage.extend(assignment.coverage);
    }
    Ok(coverage)
}

fn build_review_request(
    run_id: &str,
    participant_assignment_id: &str,
    assignment: &ProseAssignment,
    review_inputs: &[ArtifactIdentity],
    parts: ReviewRequestParts,
    run_directory: &Path,
    repository: &Path,
) -> Result<ReviewRequestArtifacts> {
    let submission_template = write_review_template(
        run_id,
        participant_assignment_id,
        assignment,
        parts.candidate.as_ref(),
        run_directory,
    )?;
    let mut bound_inputs = review_inputs.to_vec();
    bound_inputs.push(submission_template.clone());
    let binding = ReviewPacketBinding {
        schema: PROSE_PACKET_SCHEMA.to_owned(),
        assignment: review_assignment_projection(participant_assignment_id, assignment),
        inputs: bound_inputs,
        mechanical_observations: parts.mechanical_observations.clone(),
    };
    let binding_path = run_directory.join("review/packet.json");
    atomic_write_json(&binding_path, &binding)?;
    let binding_artifact = artifact_identity(run_directory, &binding_path)?;
    let request = ReviewRequest {
        schema: REVIEW_REQUEST_SCHEMA.to_owned(),
        run_id: run_id.to_owned(),
        assignment_id: participant_assignment_id.to_owned(),
        review_packet_sha256: binding_artifact.sha256.clone(),
        artifact_root: ".".to_owned(),
        article: assignment.article.clone(),
        instructions: parts.instructions,
        context: parts.context,
        sources: parts.sources,
        candidate: parts.candidate,
        mechanical_observations: parts.mechanical_observations,
        finding_tag_vocabulary: assignment.finding_tag_vocabulary.clone(),
        review_axes: assignment.review_axes.clone(),
        axis_verdict_values: vec![
            AxisVerdict::Pass,
            AxisVerdict::Concern,
            AxisVerdict::Fail,
            AxisVerdict::NotAssessed,
        ],
        submission_schema: REVIEW_SUBMISSION_SCHEMA.to_owned(),
        submission_template,
    };
    let request_path = run_directory.join("review/request.json");
    atomic_write_json(&request_path, &request)?;
    let request_artifact = artifact_identity(run_directory, &request_path)?;
    let export = write_participant_export(
        run_id,
        "reviewer",
        repository,
        run_directory,
        &request_path,
        &binding.inputs,
        &request.mechanical_observations,
    )?;
    Ok(ReviewRequestArtifacts {
        packet: binding_artifact,
        request: request_artifact,
        export,
    })
}

pub(crate) fn review_assignment_projection(
    participant_assignment_id: &str,
    assignment: &ProseAssignment,
) -> ReviewAssignmentProjection {
    ReviewAssignmentProjection {
        participant_assignment_id: participant_assignment_id.to_owned(),
        article: assignment.article.clone(),
        finding_tag_vocabulary: assignment.finding_tag_vocabulary.clone(),
        review_axes: assignment.review_axes.clone(),
    }
}

fn copy_artifact_to_export(
    run_directory: &Path,
    export_root: &Path,
    artifact: &ArtifactIdentity,
) -> Result<()> {
    let source = resolve_output_path(run_directory, &artifact.locator)?;
    let destination = resolve_output_path(export_root, &artifact.locator)?;
    atomic_write(&destination, &fs::read(source)?)
}

fn copy_output_to_export(
    run_directory: &Path,
    export_root: &Path,
    artifact: &OutputArtifact,
) -> Result<()> {
    let source = resolve_output_path(run_directory, &artifact.locator)?;
    let destination = resolve_output_path(export_root, &artifact.locator)?;
    atomic_write(&destination, &fs::read(source)?)
}

fn author_visible_locator(locator: &str) -> bool {
    locator == "inputs/assignment.json"
        || locator.starts_with("inputs/author-instructions/")
        || locator.starts_with("inputs/context/")
        || locator.starts_with("inputs/sources/")
        || locator == "author/submission-template.json"
        || locator == "author/claim-map-template.json"
}

fn review_visible_inputs(
    inputs: &[ArtifactIdentity],
    candidate: Option<&ArtifactIdentity>,
) -> Vec<ArtifactIdentity> {
    let mut visible = inputs
        .iter()
        .filter(|artifact| {
            artifact.locator.starts_with("inputs/review-instructions/")
                || artifact.locator.starts_with("inputs/context/")
                || artifact.locator.starts_with("inputs/sources/")
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut locators = visible
        .iter()
        .map(|artifact| artifact.locator.clone())
        .collect::<BTreeSet<_>>();
    for artifact in [candidate].into_iter().flatten() {
        if locators.insert(artifact.locator.clone()) {
            visible.push(artifact.clone());
        }
    }
    visible
}

fn write_participant_export(
    run_id: &str,
    stage: &str,
    repository: &Path,
    run_directory: &Path,
    retained_request_path: &Path,
    inputs: &[ArtifactIdentity],
    observations: &[MechanicalObservation],
) -> Result<ParticipantExport> {
    let repository = fs::canonicalize(repository)?;
    let run_directory = fs::canonicalize(run_directory)?;
    let export_parent = std::env::temp_dir().join("wikitest-participant-exports");
    fs::create_dir_all(&export_parent)?;
    let export_parent = fs::canonicalize(&export_parent)?;
    if export_parent.starts_with(&repository)
        || export_parent.starts_with(&run_directory)
        || run_directory.starts_with(&export_parent)
    {
        bail!("participant export parent is not isolated from repository and holdout roots");
    }
    validate_participant_stage(stage)?;
    let export_root = export_parent.join(format!("{run_id}-{stage}"));
    fs::create_dir(&export_root).with_context(|| {
        format!(
            "participant export already exists or could not be created: {}",
            export_root.display()
        )
    })?;
    let request_locator = match stage {
        "author" => "author/request.json",
        "reviewer" => "request.json",
        _ => bail!("unsupported participant export stage {stage:?}"),
    };
    let request_path = resolve_output_path(&export_root, request_locator)?;
    atomic_write(&request_path, &fs::read(retained_request_path)?)?;
    fs::create_dir(export_root.join("output"))?;
    for input in inputs {
        copy_artifact_to_export(&run_directory, &export_root, input)?;
    }
    for observation in observations {
        copy_output_to_export(&run_directory, &export_root, &observation.stdout)?;
        copy_output_to_export(&run_directory, &export_root, &observation.stderr)?;
    }
    let request = artifact_identity(&export_root, &request_path)?;
    Ok(ParticipantExport {
        root: participant_export_token(stage)?,
        request,
        output_directory: "output".to_owned(),
    })
}

fn validate_participant_stage(stage: &str) -> Result<()> {
    if matches!(stage, "author" | "reviewer") {
        Ok(())
    } else {
        bail!("unsupported participant export stage {stage:?}")
    }
}

fn participant_export_token(stage: &str) -> Result<String> {
    validate_participant_stage(stage)?;
    Ok(format!("{PARTICIPANT_EXPORT_ROOT_PREFIX}{stage}"))
}

pub fn operational_participant_export_root(
    run_id: &str,
    export: &ParticipantExport,
) -> Result<PathBuf> {
    let stage = export
        .root
        .strip_prefix(PARTICIPANT_EXPORT_ROOT_PREFIX)
        .context("participant export root is not a stable typed token")?;
    validate_participant_stage(stage)?;
    let leaf = format!("{run_id}-{stage}");
    if Path::new(&leaf).components().count() != 1 {
        bail!("participant export run identity is not a single safe path component");
    }
    Ok(std::env::temp_dir()
        .join("wikitest-participant-exports")
        .join(leaf))
}

fn write_author_templates(
    run_id: &str,
    participant_assignment_id: &str,
    assignment: &ProseAssignment,
    run_directory: &Path,
) -> Result<(ArtifactIdentity, ArtifactIdentity)> {
    let decision = assignment
        .allowed_decisions
        .first()
        .copied()
        .context("authoring assignment has no allowed decision")?;
    let mut submission = serde_json::json!({
        "schema": AUTHOR_SUBMISSION_SCHEMA,
        "run_id": run_id,
        "assignment_id": participant_assignment_id,
        "packet_sha256": "replace-with-packet-sha256-from-request",
        "author": {
            "id": "replace-with-stable-participant-id",
            "display_name": "Replace with author name",
            "kind": "agent",
            "execution": {
                "provider": "Replace with provider",
                "model": "Replace with exact model",
                "harness": "Replace with harness name",
                "harness_version": "Replace with harness version",
                "invocation_id": "Replace with invocation identifier",
                "reasoning_effort": "Replace or remove when unavailable",
                "access": {
                    "network": false,
                    "ambient_repository": false,
                    "tools": []
                },
                "metrics": {
                    "duration_ms": 1
                }
            }
        },
        "decision": decision,
        "notes": "Describe the source work and drafting decision.",
        "holds": []
    });
    if decision == crate::prose_model::ArticleDecision::Article {
        submission["candidate"] = serde_json::json!({
            "locator": "candidate.wiki",
            "sha256": "replace-with-candidate-sha256"
        });
        submission["claim_map"] = serde_json::json!({
            "locator": "claim-map.json",
            "sha256": "replace-with-claim-map-sha256"
        });
    }
    let submission_path = run_directory.join("author/submission-template.json");
    atomic_write_json(&submission_path, &submission)?;
    let source_id = assignment
        .sources
        .first()
        .map(|source| source.id.as_str())
        .unwrap_or("source-id");
    let claim_map = serde_json::json!({
        "schema": CLAIM_MAP_SCHEMA,
        "article_title": assignment.article.title,
        "candidate_sha256": "replace-with-candidate-sha256",
        "claims": [{
            "id": "c1",
            "claim": "Replace with one material claim from the candidate.",
            "evidence": [{
                "source_id": source_id,
                "locator": "Replace with an entailing passage, record, or timestamp."
            }],
            "qualification": "Replace or remove when no qualification is needed."
        }],
        "holds": []
    });
    let claim_map_path = run_directory.join("author/claim-map-template.json");
    atomic_write_json(&claim_map_path, &claim_map)?;
    Ok((
        artifact_identity(run_directory, &submission_path)?,
        artifact_identity(run_directory, &claim_map_path)?,
    ))
}

fn write_review_template(
    run_id: &str,
    participant_assignment_id: &str,
    assignment: &ProseAssignment,
    candidate: Option<&ArtifactIdentity>,
    run_directory: &Path,
) -> Result<ArtifactIdentity> {
    let axes = assignment
        .review_axes
        .iter()
        .map(|axis| {
            serde_json::json!({
                "axis": axis,
                "verdict": "not_assessed",
                "rationale": "Replace with evidence for this axis."
            })
        })
        .collect::<Vec<_>>();
    let template = serde_json::json!({
        "schema": REVIEW_SUBMISSION_SCHEMA,
        "run_id": run_id,
        "assignment_id": participant_assignment_id,
        "review_packet_sha256": "replace-with-review-packet-sha256-from-request",
        "candidate_sha256": candidate.map(|artifact| artifact.sha256.as_str()),
        "finding_tag_vocabulary": assignment.finding_tag_vocabulary,
        "reviewer": {
            "id": "replace-with-stable-participant-id",
            "display_name": "Replace with reviewer name",
            "kind": "agent",
            "execution": {
                "provider": "Replace with provider",
                "model": "Replace with exact model",
                "harness": "Replace with harness name",
                "harness_version": "Replace with harness version",
                "invocation_id": "Replace with invocation identifier",
                "reasoning_effort": "Replace or remove when unavailable",
                "access": {
                    "network": false,
                    "ambient_repository": false,
                    "tools": []
                },
                "metrics": {
                    "duration_ms": 1
                }
            }
        },
        "scope": "complete",
        "reader_verdict": "Would the intended reader want to read this article, and why?",
        "source_verdict": "complete",
        "disposition": "revise",
        "findings": [{
            "id": "finding-1",
            "severity": "p2",
            "location": "Replace with the smallest useful location.",
            "problem": "Replace with the exact defect, or remove this example when there are no findings.",
            "evidence": "Cite the candidate and source packet evidence for the diagnosis.",
            "impact": "Explain the reader or publication impact.",
            "repair_direction": "Give a concrete repair direction.",
            "basis": "verified",
            "tags": []
        }],
        "observations": [{
            "tag": "replace-with-specific-observation-tag",
            "evidence": "Record a non-finding control or positive observation, or remove this example."
        }],
        "axes": axes,
        "residual_risk": "State what still requires human judgment, or say that none was identified."
    });
    let path = run_directory.join("review/submission-template.json");
    atomic_write_json(&path, &template)?;
    artifact_identity(run_directory, &path)
}

fn snapshot_bound_inputs(
    values: &[BoundInput],
    assignment_root: &Path,
    repository_root: &Path,
    run_directory: &Path,
    destination_root: &str,
    inputs: &mut Vec<ArtifactIdentity>,
) -> Result<Vec<PacketInput>> {
    values
        .iter()
        .map(|input| {
            let source_root = match input.root {
                crate::prose_model::InputRoot::Assignment => assignment_root,
                crate::prose_model::InputRoot::Repository => repository_root,
            };
            let destination = retained_bound_input_locator(destination_root, input);
            let artifact = snapshot_file(
                source_root,
                &input.locator,
                &input.sha256,
                run_directory,
                &destination,
            )?;
            inputs.push(artifact.clone());
            Ok(PacketInput {
                id: input.id.clone(),
                artifact,
            })
        })
        .collect()
}

fn snapshot_sources(
    values: &[SourceInput],
    source_root: &Path,
    run_directory: &Path,
    inputs: &mut Vec<ArtifactIdentity>,
) -> Result<Vec<PacketSource>> {
    values
        .iter()
        .map(|source| {
            let destination = format!(
                "inputs/sources/{}{}",
                source.id,
                extension_for_locator(&source.locator)
            );
            let artifact = snapshot_file(
                source_root,
                &source.locator,
                &source.sha256,
                run_directory,
                &destination,
            )?;
            inputs.push(artifact.clone());
            Ok(PacketSource {
                id: source.id.clone(),
                title: source.title.clone(),
                role: source.role,
                citation: source.citation.clone(),
                artifact,
            })
        })
        .collect()
}

fn snapshot_single_input(
    input: &BoundInput,
    source_root: &Path,
    run_directory: &Path,
    destination_root: &str,
) -> Result<ArtifactIdentity> {
    let destination = format!(
        "{destination_root}/{}{}",
        input.id,
        extension_for_locator(&input.locator)
    );
    snapshot_file(
        source_root,
        &input.locator,
        &input.sha256,
        run_directory,
        &destination,
    )
}

fn snapshot_file(
    source_root: &Path,
    locator: &str,
    expected_sha256: &str,
    run_directory: &Path,
    destination: &str,
) -> Result<ArtifactIdentity> {
    let source = resolve_existing_plain_file(source_root, locator)?;
    let bytes = fs::read(&source)?;
    let observed = sha256_bytes(&bytes);
    if observed != expected_sha256 {
        bail!("input digest mismatch for {locator}: got {observed}, expected {expected_sha256}");
    }
    let destination = resolve_output_path(run_directory, destination)?;
    atomic_write(&destination, &bytes)?;
    artifact_identity(run_directory, &destination)
}

fn snapshot_submission_file(
    reference: &crate::prose_model::SubmissionFile,
    source_root: &Path,
    run_directory: &Path,
    destination: &str,
) -> Result<ArtifactIdentity> {
    snapshot_file(
        source_root,
        &reference.locator,
        &reference.sha256,
        run_directory,
        destination,
    )
}

fn packet_inputs_for_prefix(
    artifacts: &[ArtifactIdentity],
    prefix: &str,
    declared: &[BoundInput],
) -> Result<Vec<PacketInput>> {
    declared
        .iter()
        .map(|input| {
            let expected_locator = retained_bound_input_locator(prefix, input);
            let artifact = artifacts
                .iter()
                .find(|artifact| artifact.locator == expected_locator)
                .with_context(|| format!("missing retained input '{}'", input.id))?
                .clone();
            Ok(PacketInput {
                id: input.id.clone(),
                artifact,
            })
        })
        .collect()
}

fn packet_sources_from_receipt(
    artifacts: &[ArtifactIdentity],
    sources: &[SourceInput],
) -> Result<Vec<PacketSource>> {
    sources
        .iter()
        .map(|source| {
            let expected_locator =
                retained_input_locator("inputs/sources", &source.id, &source.locator);
            let artifact = artifacts
                .iter()
                .find(|artifact| artifact.locator == expected_locator)
                .with_context(|| format!("missing retained source '{}'", source.id))?
                .clone();
            Ok(PacketSource {
                id: source.id.clone(),
                title: source.title.clone(),
                role: source.role,
                citation: source.citation.clone(),
                artifact,
            })
        })
        .collect()
}

fn retained_input_locator(prefix: &str, id: &str, source_locator: &str) -> String {
    format!(
        "{}/{}{}",
        prefix.trim_end_matches('/'),
        id,
        extension_for_locator(source_locator)
    )
}

fn retained_bound_input_locator(prefix: &str, input: &BoundInput) -> String {
    let components = Path::new(&input.locator)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    if let Some(index) = components
        .windows(2)
        .position(|pair| pair == ["agent-pack", "skills"])
        && components.len() > index + 3
    {
        return format!(
            "{}/{}",
            prefix.trim_end_matches('/'),
            components[index + 2..].join("/")
        );
    }
    retained_input_locator(prefix, &input.id, &input.locator)
}

fn run_mechanical_observations(
    candidate: &ArtifactIdentity,
    title: &str,
    run_directory: &Path,
    options: &ProseOptions,
) -> Result<Vec<MechanicalObservation>> {
    let run_name = run_directory
        .file_name()
        .and_then(|name| name.to_str())
        .context("prose run directory name is not UTF-8")?;
    let project = create_execution_workspace(&format!("{run_name}-mechanical-{}", unix_ms()?))?;
    validate_stored_execution_root(&project, &options.repository, run_directory)?;
    let candidate_path = resolve_output_path(run_directory, &candidate.locator)?;
    let candidate_bytes = fs::read(candidate_path)?;
    let mut init_execution_argv = mechanical_runtime_argv(&project);
    init_execution_argv.extend(["init".to_owned(), "--no-network".to_owned()]);
    let mut init_evidence_argv = canonical_mechanical_runtime_argv();
    init_evidence_argv.extend(["init".to_owned(), "--no-network".to_owned()]);
    let init = run_observation(
        "init",
        &init_execution_argv,
        &init_evidence_argv,
        &project,
        run_directory,
        options,
    )?;
    if init.exit_code != Some(0) || init.timed_out || init.stdout.truncated || init.stderr.truncated
    {
        bail!("isolated mechanical project initialization did not complete successfully");
    }
    let mut observations = vec![init];
    let draft_path = project.join(".wikitool/drafts/candidate.wiki");
    atomic_write(&draft_path, &candidate_bytes)?;
    let mut lint_execution_argv = mechanical_runtime_argv(&project);
    lint_execution_argv.extend([
        "article".to_owned(),
        "lint".to_owned(),
        ".wikitool/drafts/candidate.wiki".to_owned(),
        "--title".to_owned(),
        title.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]);
    let mut lint_evidence_argv = canonical_mechanical_runtime_argv();
    lint_evidence_argv.extend([
        "article".to_owned(),
        "lint".to_owned(),
        ".wikitool/drafts/candidate.wiki".to_owned(),
        "--title".to_owned(),
        title.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]);
    let mut lint = run_observation(
        "lint",
        &lint_execution_argv,
        &lint_evidence_argv,
        &project,
        run_directory,
        options,
    )?;
    let (observed_candidate_sha256, observed_candidate_bytes) = sha256_file(&draft_path)?;
    if observed_candidate_sha256 != candidate.sha256 || observed_candidate_bytes != candidate.bytes
    {
        bail!("mechanical lint input changed while the observation was running");
    }
    lint.input = Some(candidate.clone());
    observations.push(lint);
    Ok(observations)
}

fn mechanical_runtime_argv(project: &Path) -> Vec<String> {
    vec![
        "--project-root".to_owned(),
        project.to_string_lossy().into_owned(),
        "--data-dir".to_owned(),
        project
            .join(".wikitool/data")
            .to_string_lossy()
            .into_owned(),
        "--config".to_owned(),
        project
            .join(".wikitool/config.toml")
            .to_string_lossy()
            .into_owned(),
    ]
}

fn canonical_mechanical_runtime_argv() -> Vec<String> {
    vec![
        "--project-root".to_owned(),
        MECHANICAL_ROOT_TOKEN.to_owned(),
        "--data-dir".to_owned(),
        MECHANICAL_DATA_TOKEN.to_owned(),
        "--config".to_owned(),
        MECHANICAL_CONFIG_TOKEN.to_owned(),
    ]
}

fn run_observation(
    id: &str,
    execution_argv: &[String],
    evidence_argv: &[String],
    cwd: &Path,
    run_directory: &Path,
    options: &ProseOptions,
) -> Result<MechanicalObservation> {
    let stdout_path = run_directory.join(format!("mechanical/{id}.stdout.txt"));
    let stderr_path = run_directory.join(format!("mechanical/{id}.stderr.txt"));
    let raw_directory = cwd.join(".wikitest-raw/mechanical");
    fs::create_dir_all(&raw_directory)?;
    let raw_stdout_path = raw_directory.join(format!("{id}.stdout.txt"));
    let raw_stderr_path = raw_directory.join(format!("{id}.stderr.txt"));
    let outcome = run_bounded(
        &options.wikitool,
        execution_argv,
        cwd,
        &BTreeMap::new(),
        PROSE_PROCESS_TIMEOUT,
        options.maximum_output_bytes,
        &raw_stdout_path,
        &raw_stderr_path,
    )?;
    Ok(MechanicalObservation {
        argv: evidence_argv.to_vec(),
        input: None,
        exit_code: outcome.status.code(),
        timed_out: outcome.timed_out,
        duration_ms: outcome.duration_ms,
        stdout: canonicalized_output_artifact(
            run_directory,
            &stdout_path,
            &outcome.stdout,
            cwd,
            &options.wikitool,
        )?,
        stderr: canonicalized_output_artifact(
            run_directory,
            &stderr_path,
            &outcome.stderr,
            cwd,
            &options.wikitool,
        )?,
    })
}

fn tool_identity(options: &ProseOptions, run_directory: &Path) -> Result<ToolIdentity> {
    let (sha256, _) = sha256_file(&options.wikitool)?;
    let version = probe_version(
        &options.wikitool,
        &options.repository,
        &run_directory.join("tool/version.stdout.txt"),
        &run_directory.join("tool/version.stderr.txt"),
    )?;
    Ok(ToolIdentity {
        locator: repository_binary_locator(
            &options.wikitool,
            &options.repository,
            "configured Wikitool",
        )?,
        sha256,
        version,
    })
}

fn canonicalized_output_artifact(
    run_directory: &Path,
    path: &Path,
    stream: &CapturedStream,
    mechanical_root: &Path,
    tool_binary: &Path,
) -> Result<OutputArtifact> {
    if stream.truncated
        || stream.sha256 != stream.stored_sha256
        || stream.observed_bytes != stream.stored_bytes
        || stream.bytes.len() as u64 != stream.stored_bytes
    {
        bail!("mechanical output must be complete before public path canonicalization");
    }
    let bytes = canonicalize_runner_owned_paths(&stream.bytes, mechanical_root, tool_binary)?;
    atomic_write(path, &bytes)?;
    let sha256 = sha256_bytes(&bytes);
    let bytes = bytes.len() as u64;
    Ok(OutputArtifact {
        locator: relative_locator(run_directory, path)?,
        sha256: sha256.clone(),
        stored_sha256: sha256,
        observed_bytes: bytes,
        stored_bytes: bytes,
        truncated: false,
    })
}

fn canonicalize_runner_owned_paths(
    bytes: &[u8],
    mechanical_root: &Path,
    tool_binary: &Path,
) -> Result<Vec<u8>> {
    let execution_workspaces = mechanical_root
        .parent()
        .context("mechanical workspace has no runner-owned parent")?;
    let paths = [
        (
            mechanical_root.join(".wikitool/config.toml"),
            MECHANICAL_CONFIG_TOKEN,
        ),
        (
            mechanical_root.join(".wikitool/data"),
            MECHANICAL_DATA_TOKEN,
        ),
        (mechanical_root.to_path_buf(), MECHANICAL_ROOT_TOKEN),
        (
            execution_workspaces.to_path_buf(),
            EXECUTION_WORKSPACES_TOKEN,
        ),
        (tool_binary.to_path_buf(), TOOL_BINARY_TOKEN),
    ];
    canonicalize_exact_paths(bytes, &paths)
}

pub(crate) fn evaluate_oracle(
    oracle: &crate::prose_model::ReviewOracle,
    submission: &ReviewSubmission,
) -> OracleEvaluation {
    let tags = submission
        .findings
        .iter()
        .flat_map(|finding| finding.tags.iter().cloned())
        .collect::<BTreeSet<_>>();
    let missing_required_tags = oracle
        .required_finding_tags
        .iter()
        .filter(|tag| !tags.contains(tag.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let present_forbidden_tags = oracle
        .forbidden_finding_tags
        .iter()
        .filter(|tag| tags.contains(tag.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let axis_verdicts = submission
        .axes
        .iter()
        .map(|axis| (axis.axis.as_str(), axis.verdict))
        .collect::<BTreeMap<_, _>>();
    let failed_axis_expectations = oracle
        .axis_expectations
        .iter()
        .filter(|expectation| {
            axis_verdicts
                .get(expectation.axis.as_str())
                .is_none_or(|verdict| !expectation.allowed_verdicts.contains(verdict))
        })
        .map(|expectation| expectation.axis.clone())
        .collect::<Vec<_>>();
    let disposition_allowed = oracle
        .allowed_dispositions
        .contains(&submission.disposition);
    OracleEvaluation {
        passed: missing_required_tags.is_empty()
            && present_forbidden_tags.is_empty()
            && failed_axis_expectations.is_empty()
            && disposition_allowed,
        missing_required_tags,
        present_forbidden_tags,
        failed_axis_expectations,
        disposition_allowed,
    }
}

pub(crate) fn prose_coverage_status(
    receipt: &ProseReceipt,
    assignment: &ProseAssignment,
    run_directory: &Path,
) -> Result<ProseCoverageStatus> {
    verify_stage_semantics(receipt, assignment, run_directory)?;
    let author_complete = if assignment.mode == ProseMode::Authoring {
        match &receipt.author {
            Some(author)
                if author.decision == crate::prose_model::ArticleDecision::Article
                    && author.candidate.is_some()
                    && author.claim_map.is_some()
                    && author.holds.is_empty()
                    && author.mechanical_observations.len() >= 2
                    && author.mechanical_observations.iter().all(|observation| {
                        observation.exit_code == Some(0)
                            && !observation.timed_out
                            && !observation.stdout.truncated
                            && !observation.stderr.truncated
                    }) =>
            {
                let claim_map_artifact = author
                    .claim_map
                    .as_ref()
                    .context("authoring stage has no claim map")?;
                let claim_map_path =
                    resolve_output_path(run_directory, &claim_map_artifact.locator)?;
                let claim_map: ClaimMap = serde_json::from_slice(&fs::read(claim_map_path)?)
                    .context("invalid retained claim map")?;
                claim_map.holds.is_empty()
            }
            _ => false,
        }
    } else {
        true
    };
    Ok(classify_prose_coverage(DemonstrationEvidence {
        mode: assignment.mode,
        receipt_status: receipt.status,
        oracle_passed: receipt
            .review
            .as_ref()
            .map(|review| review.oracle.as_ref().map(|oracle| oracle.passed)),
        review_complete: receipt.review.as_ref().is_some_and(|review| {
            review.scope == ReviewScope::Complete
                && review.source_verdict == SourceVerdict::Complete
        }),
        review_disposition: receipt.review.as_ref().map(|review| review.disposition),
        author_complete,
    }))
}

#[derive(Debug, Clone, Copy)]
struct DemonstrationEvidence {
    mode: ProseMode,
    receipt_status: ProseRunStatus,
    oracle_passed: Option<Option<bool>>,
    review_complete: bool,
    review_disposition: Option<ReviewDisposition>,
    author_complete: bool,
}

fn classify_prose_coverage(evidence: DemonstrationEvidence) -> ProseCoverageStatus {
    let Some(oracle_passed) = evidence.oracle_passed else {
        return ProseCoverageStatus::AwaitingReview;
    };
    let Some(oracle_passed) = oracle_passed else {
        return ProseCoverageStatus::MissingOracle;
    };
    if !oracle_passed {
        return ProseCoverageStatus::OracleFailed;
    }
    if !evidence.review_complete {
        return ProseCoverageStatus::ReviewIncomplete;
    }
    if evidence.mode == ProseMode::Review {
        return ProseCoverageStatus::Demonstrated;
    }
    if !evidence.author_complete {
        return ProseCoverageStatus::AuthoringIncomplete;
    }
    if evidence.receipt_status != ProseRunStatus::ReviewedAccept
        || evidence.review_disposition != Some(ReviewDisposition::Accept)
    {
        return ProseCoverageStatus::AuthoringRejected;
    }
    ProseCoverageStatus::Demonstrated
}

fn verify_stage_semantics(
    receipt: &ProseReceipt,
    assignment: &ProseAssignment,
    run_directory: &Path,
) -> Result<()> {
    let author_submission = match (&receipt.author, assignment.mode) {
        (Some(stage), ProseMode::Authoring) => {
            let path = resolve_output_path(run_directory, &stage.submission.locator)?;
            let submission: AuthorSubmission = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained author submission")?;
            submission.validate()?;
            if submission.run_id != receipt.run_id
                || submission.assignment_id != receipt.participant_assignment_id
                || submission.packet_sha256 != receipt.packet.sha256
                || !assignment.allowed_decisions.contains(&submission.decision)
                || submission.decision != stage.decision
                || serde_json::to_value(&submission.author)? != serde_json::to_value(&stage.author)?
                || submission.notes != stage.notes
                || submission.holds != stage.holds
                || submission
                    .candidate
                    .as_ref()
                    .map(|file| file.sha256.as_str())
                    != stage.candidate.as_ref().map(|file| file.sha256.as_str())
                || submission
                    .claim_map
                    .as_ref()
                    .map(|file| file.sha256.as_str())
                    != stage.claim_map.as_ref().map(|file| file.sha256.as_str())
            {
                bail!("retained author submission does not derive the author stage receipt");
            }
            if let (Some(candidate), Some(claim_map)) = (&stage.candidate, &stage.claim_map) {
                let path = resolve_output_path(run_directory, &claim_map.locator)?;
                let claim_map: ClaimMap = serde_json::from_slice(&fs::read(path)?)
                    .context("invalid retained claim map")?;
                claim_map.validate(assignment, &candidate.sha256)?;
            }
            Some(submission)
        }
        (None, ProseMode::Authoring) => None,
        (None, ProseMode::Review) => None,
        (Some(_), ProseMode::Review) => {
            bail!("review-only assignment unexpectedly contains an author stage")
        }
    };

    match &receipt.review {
        Some(stage) => {
            let packet = receipt
                .review_packet
                .as_ref()
                .context("review stage has no review packet")?;
            let path = resolve_output_path(run_directory, &stage.submission.locator)?;
            let submission: ReviewSubmission = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained review submission")?;
            submission.validate(assignment)?;
            let expected_candidate = receipt
                .author
                .as_ref()
                .and_then(|author| author.candidate.as_ref())
                .or_else(|| {
                    receipt.inputs.iter().find(|artifact| {
                        artifact.locator.starts_with("inputs/candidate/")
                            || artifact.locator == "inputs/candidate"
                    })
                })
                .map(|artifact| artifact.sha256.as_str());
            ensure_independent_reviewer(
                receipt.author.as_ref().map(|author| &author.author),
                &submission.reviewer,
            )?;
            if submission.run_id != receipt.run_id
                || submission.assignment_id != receipt.participant_assignment_id
                || submission.review_packet_sha256 != packet.sha256
                || submission.candidate_sha256.as_deref() != expected_candidate
                || serde_json::to_value(&submission.reviewer)?
                    != serde_json::to_value(&stage.reviewer)?
                || submission.scope != stage.scope
                || submission.reader_verdict != stage.reader_verdict
                || submission.source_verdict != stage.source_verdict
                || submission.disposition != stage.disposition
                || submission.findings.len() != stage.finding_count
                || submission.residual_risk != stage.residual_risk
            {
                bail!("retained review submission does not derive the review stage receipt");
            }
            let expected_oracle = assignment
                .oracle
                .as_ref()
                .map(|oracle| evaluate_oracle(oracle, &submission));
            if serde_json::to_value(&expected_oracle)? != serde_json::to_value(&stage.oracle)? {
                bail!("review oracle result does not match the held-out assignment");
            }
            let expected_status = match submission.disposition {
                ReviewDisposition::Accept => ProseRunStatus::ReviewedAccept,
                ReviewDisposition::Revise => ProseRunStatus::ReviewedRevise,
                ReviewDisposition::Block => ProseRunStatus::ReviewedBlock,
            };
            if receipt.status != expected_status || !receipt.evaluation_complete {
                bail!("terminal review status does not match the retained submission");
            }
        }
        None => {
            let expected_status =
                if author_submission.is_some() || assignment.mode == ProseMode::Review {
                    ProseRunStatus::AwaitingReview
                } else {
                    ProseRunStatus::AwaitingAuthor
                };
            if receipt.status != expected_status || receipt.evaluation_complete {
                bail!("pending prose status does not match retained stages");
            }
        }
    }
    Ok(())
}

fn ensure_independent_reviewer(
    author: Option<&crate::prose_model::Participant>,
    reviewer: &crate::prose_model::Participant,
) -> Result<()> {
    if author.is_some_and(|author| author.id == reviewer.id) {
        bail!(
            "reviewer '{}' is the recorded author; authoring runs require a distinct reviewer identity",
            reviewer.id
        );
    }
    Ok(())
}

fn load_prose_run(run: &Path) -> Result<(ProseReceipt, PathBuf, PathBuf, ProseAssignment)> {
    let candidate = if run.is_dir() {
        run.join("receipt.json")
    } else {
        run.to_path_buf()
    };
    let receipt_path = fs::canonicalize(&candidate)
        .with_context(|| format!("failed to resolve prose run {}", candidate.display()))?;
    let run_directory = receipt_path
        .parent()
        .context("prose receipt has no parent")?
        .to_path_buf();
    let receipt: ProseReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("invalid strict prose receipt JSON")?;
    if receipt.schema != PROSE_RECEIPT_SCHEMA {
        bail!("unsupported prose receipt schema '{}'", receipt.schema);
    }
    let assignment_path = resolve_output_path(&run_directory, "inputs/assignment.json")?;
    let assignment: ProseAssignment = serde_json::from_slice(&fs::read(assignment_path)?)
        .context("invalid retained prose assignment")?;
    assignment.validate()?;
    Ok((receipt, receipt_path, run_directory, assignment))
}

fn load_authority_assignment(
    receipt: &ProseReceipt,
    run_directory: &Path,
) -> Result<ProseAssignment> {
    verify_artifact(run_directory, &receipt.authority)?;
    let path = resolve_output_path(run_directory, &receipt.authority.locator)?;
    let assignment: ProseAssignment = serde_json::from_slice(&fs::read(&path)?)
        .context("invalid retained assignment authority")?;
    assignment.validate()?;
    if assignment.id != receipt.public_assignment.id
        || assignment.title != receipt.public_assignment.title
        || assignment.mode != receipt.public_assignment.mode
        || assignment.coverage != receipt.public_assignment.coverage
    {
        bail!("prose assignment authority identity does not match the receipt");
    }
    Ok(assignment)
}

pub(crate) fn verify_current_receipt(
    repository: &Path,
    run_directory: &Path,
    receipt: &ProseReceipt,
) -> Result<()> {
    verify_recorded_identity(repository, &receipt.driver, "Wikitest driver")?;
    verify_recorded_identity(repository, &receipt.tool, "Wikitool")?;
    verify_artifact(run_directory, &receipt.authority)?;
    verify_artifact(run_directory, &receipt.packet)?;
    for input in &receipt.inputs {
        verify_artifact(run_directory, input)?;
    }
    let public_assignment_artifact = receipt
        .inputs
        .iter()
        .find(|artifact| artifact.locator == "inputs/assignment.json")
        .context("retained public assignment artifact is missing")?;
    if receipt.public_assignment.locator != public_assignment_artifact.locator
        || receipt.public_assignment.sha256 != public_assignment_artifact.sha256
    {
        bail!("public assignment identity does not bind the oracle-redacted artifact");
    }
    for artifact in [
        receipt.author_request.as_ref(),
        receipt.review_packet.as_ref(),
        receipt.review_request.as_ref(),
        receipt.author.as_ref().map(|author| &author.submission),
        receipt
            .author
            .as_ref()
            .and_then(|author| author.candidate.as_ref()),
        receipt
            .author
            .as_ref()
            .and_then(|author| author.claim_map.as_ref()),
        receipt.review.as_ref().map(|review| &review.submission),
    ]
    .into_iter()
    .flatten()
    {
        verify_artifact(run_directory, artifact)?;
    }
    if let Some(author) = &receipt.author {
        verify_observations(run_directory, &author.mechanical_observations)?;
    }
    let assignment = load_authority_assignment(receipt, run_directory)?;
    verify_request_semantics(receipt, &assignment, run_directory)?;
    match (&receipt.author_request, &receipt.author_export) {
        (Some(request), Some(export)) => {
            let visible_inputs = receipt
                .inputs
                .iter()
                .filter(|artifact| author_visible_locator(&artifact.locator))
                .cloned()
                .collect::<Vec<_>>();
            verify_participant_export(
                repository,
                run_directory,
                &receipt.run_id,
                ParticipantExportEvidence {
                    expected_stage: "author",
                    retained_request: request,
                    export,
                    inputs: &visible_inputs,
                    observations: &[],
                    external_required: receipt.author.is_none(),
                },
            )?;
        }
        (None, None) => {}
        _ => bail!("author request and isolated export must be recorded together"),
    }
    match (
        &receipt.review_request,
        &receipt.review_export,
        &receipt.review_packet,
    ) {
        (Some(request), Some(export), Some(packet)) => {
            let path = resolve_output_path(run_directory, &packet.locator)?;
            let binding: ReviewPacketBinding = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained review packet")?;
            verify_participant_export(
                repository,
                run_directory,
                &receipt.run_id,
                ParticipantExportEvidence {
                    expected_stage: "reviewer",
                    retained_request: request,
                    export,
                    inputs: &binding.inputs,
                    observations: &binding.mechanical_observations,
                    external_required: receipt.review.is_none(),
                },
            )?;
        }
        (None, None, None) => {}
        _ => bail!("review request, packet, and isolated export must be recorded together"),
    }
    let public_path = resolve_output_path(run_directory, "inputs/assignment.json")?;
    let public_bytes = fs::read(public_path)?;
    let public_assignment: ProseAssignment =
        serde_json::from_slice(&public_bytes).context("invalid author-visible assignment")?;
    if public_bytes != oracle_redacted_assignment_bytes(&assignment)? {
        bail!("author-visible assignment is not the exact oracle-redacted authority");
    }
    debug_assert!(public_assignment.oracle.is_none());
    verify_stage_semantics(receipt, &assignment, run_directory)?;
    Ok(())
}

fn verify_request_semantics(
    receipt: &ProseReceipt,
    assignment: &ProseAssignment,
    run_directory: &Path,
) -> Result<()> {
    let packet_path = resolve_output_path(run_directory, &receipt.packet.locator)?;
    let packet: PacketBinding =
        serde_json::from_slice(&fs::read(packet_path)?).context("invalid retained prose packet")?;
    let expected_packet = PacketBinding {
        schema: PROSE_PACKET_SCHEMA.to_owned(),
        assignment: receipt.public_assignment.clone(),
        inputs: receipt.inputs.clone(),
    };
    if serde_json::to_value(&packet)? != serde_json::to_value(&expected_packet)? {
        bail!("retained prose packet does not exactly bind the assignment and inputs");
    }

    match (&receipt.author_request, assignment.mode) {
        (Some(request_artifact), ProseMode::Authoring) => {
            let path = resolve_output_path(run_directory, &request_artifact.locator)?;
            let request: AuthorRequest = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained author request")?;
            let expected = AuthorRequest {
                schema: AUTHOR_REQUEST_SCHEMA.to_owned(),
                run_id: receipt.run_id.clone(),
                assignment_id: receipt.participant_assignment_id.clone(),
                packet_sha256: receipt.packet.sha256.clone(),
                artifact_root: "..".to_owned(),
                article: assignment.article.clone(),
                instructions: packet_inputs_for_prefix(
                    &receipt.inputs,
                    "inputs/author-instructions/",
                    &assignment.author_instructions,
                )?,
                context: packet_inputs_for_prefix(
                    &receipt.inputs,
                    "inputs/context/",
                    &assignment.context,
                )?,
                sources: packet_sources_from_receipt(&receipt.inputs, &assignment.sources)?,
                do_not_assert: assignment.do_not_assert.clone(),
                allowed_decisions: assignment.allowed_decisions.clone(),
                candidate_format: "MediaWiki wikitext encoded as UTF-8".to_owned(),
                claim_map_schema: CLAIM_MAP_SCHEMA.to_owned(),
                submission_schema: AUTHOR_SUBMISSION_SCHEMA.to_owned(),
                submission_template: artifact_identity(
                    run_directory,
                    &run_directory.join("author/submission-template.json"),
                )?,
                claim_map_template: artifact_identity(
                    run_directory,
                    &run_directory.join("author/claim-map-template.json"),
                )?,
            };
            if serde_json::to_value(&request)? != serde_json::to_value(&expected)? {
                bail!("retained author request does not exactly derive from assignment authority");
            }
        }
        (None, ProseMode::Review) => {}
        (None, ProseMode::Authoring) => bail!("authoring run has no author request"),
        (Some(_), ProseMode::Review) => bail!("review-only run unexpectedly has an author request"),
    }

    match (&receipt.review_packet, &receipt.review_request) {
        (Some(packet_artifact), Some(request_artifact)) => {
            let path = resolve_output_path(run_directory, &packet_artifact.locator)?;
            let binding: ReviewPacketBinding = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained review packet")?;
            verify_observations(run_directory, &binding.mechanical_observations)?;
            let candidate = receipt
                .author
                .as_ref()
                .and_then(|author| author.candidate.as_ref())
                .or_else(|| {
                    receipt.inputs.iter().find(|artifact| {
                        artifact.locator.starts_with("inputs/candidate/")
                            || artifact.locator == "inputs/candidate"
                    })
                });
            match candidate {
                Some(candidate) => verify_mechanical_observation_protocol(
                    &binding.mechanical_observations,
                    &assignment.article.title,
                    candidate,
                    run_directory,
                )?,
                None if assignment.mode == ProseMode::Authoring => {
                    if !binding.mechanical_observations.is_empty() {
                        bail!("candidate-free author decision has unexpected mechanical evidence");
                    }
                }
                None => bail!("review assignment has no candidate"),
            }
            let submission_template = artifact_identity(
                run_directory,
                &run_directory.join("review/submission-template.json"),
            )?;
            let mut expected_inputs = review_visible_inputs(&receipt.inputs, candidate);
            expected_inputs.push(submission_template.clone());
            let expected_observations = receipt.author.as_ref().map_or_else(
                || binding.mechanical_observations.clone(),
                |author| author.mechanical_observations.clone(),
            );
            let expected_binding = ReviewPacketBinding {
                schema: PROSE_PACKET_SCHEMA.to_owned(),
                assignment: review_assignment_projection(
                    &receipt.participant_assignment_id,
                    assignment,
                ),
                inputs: expected_inputs,
                mechanical_observations: expected_observations.clone(),
            };
            require_exact_review_inputs(&binding.inputs, &expected_binding.inputs)?;
            if serde_json::to_value(&binding)? != serde_json::to_value(&expected_binding)? {
                bail!("retained review packet contains inputs outside the reviewer allowlist");
            }

            let path = resolve_output_path(run_directory, &request_artifact.locator)?;
            let request: ReviewRequest = serde_json::from_slice(&fs::read(path)?)
                .context("invalid retained review request")?;
            let expected_request = ReviewRequest {
                schema: REVIEW_REQUEST_SCHEMA.to_owned(),
                run_id: receipt.run_id.clone(),
                assignment_id: receipt.participant_assignment_id.clone(),
                review_packet_sha256: packet_artifact.sha256.clone(),
                artifact_root: ".".to_owned(),
                article: assignment.article.clone(),
                instructions: packet_inputs_for_prefix(
                    &receipt.inputs,
                    "inputs/review-instructions/",
                    &assignment.review_instructions,
                )?,
                context: packet_inputs_for_prefix(
                    &receipt.inputs,
                    "inputs/context/",
                    &assignment.context,
                )?,
                sources: packet_sources_from_receipt(&receipt.inputs, &assignment.sources)?,
                candidate: candidate.cloned(),
                mechanical_observations: expected_observations,
                finding_tag_vocabulary: assignment.finding_tag_vocabulary.clone(),
                review_axes: assignment.review_axes.clone(),
                axis_verdict_values: vec![
                    AxisVerdict::Pass,
                    AxisVerdict::Concern,
                    AxisVerdict::Fail,
                    AxisVerdict::NotAssessed,
                ],
                submission_schema: REVIEW_SUBMISSION_SCHEMA.to_owned(),
                submission_template,
            };
            if serde_json::to_value(&request)? != serde_json::to_value(&expected_request)? {
                bail!("retained review request does not exactly derive from assignment authority");
            }
        }
        (None, None) if assignment.mode == ProseMode::Authoring && receipt.author.is_none() => {}
        (None, None) => bail!("review-ready run has no review packet and request"),
        _ => bail!("review packet and request must be recorded together"),
    }
    Ok(())
}

fn require_exact_review_inputs(
    observed: &[ArtifactIdentity],
    expected: &[ArtifactIdentity],
) -> Result<()> {
    if serde_json::to_value(observed)? != serde_json::to_value(expected)? {
        bail!("retained review packet contains inputs outside the reviewer allowlist");
    }
    Ok(())
}

fn verify_observations(run_directory: &Path, values: &[MechanicalObservation]) -> Result<()> {
    for observation in values {
        if let Some(input) = &observation.input {
            verify_artifact(run_directory, input)?;
        }
        verify_output(run_directory, &observation.stdout)?;
        verify_output(run_directory, &observation.stderr)?;
    }
    Ok(())
}

fn verify_mechanical_observation_protocol(
    observations: &[MechanicalObservation],
    title: &str,
    candidate: &ArtifactIdentity,
    run_directory: &Path,
) -> Result<()> {
    let [init, lint] = observations else {
        bail!("mechanical evidence must contain exactly one init and one article-lint observation");
    };
    let mut expected_init = canonical_mechanical_runtime_argv();
    expected_init.extend(["init".to_owned(), "--no-network".to_owned()]);
    if init.argv != expected_init
        || init.input.is_some()
        || init.exit_code != Some(0)
        || init.timed_out
        || init.stdout.truncated
        || init.stderr.truncated
    {
        bail!("mechanical init evidence does not match the isolated protocol");
    }

    let mut expected_lint = canonical_mechanical_runtime_argv();
    expected_lint.extend([
        "article".to_owned(),
        "lint".to_owned(),
        ".wikitool/drafts/candidate.wiki".to_owned(),
        "--title".to_owned(),
        title.to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ]);
    if lint.argv != expected_lint
        || lint.input.as_ref().map(|input| input.sha256.as_str()) != Some(candidate.sha256.as_str())
        || lint.input.as_ref().map(|input| input.locator.as_str())
            != Some(candidate.locator.as_str())
        || lint.timed_out
        || lint.stdout.truncated
        || lint.stderr.truncated
    {
        bail!("mechanical lint evidence does not match the candidate/title protocol");
    }
    let stdout_path = resolve_output_path(run_directory, &lint.stdout.locator)?;
    let report: serde_json::Value = serde_json::from_slice(&fs::read(stdout_path)?)
        .context("mechanical article lint output is not strict JSON")?;
    if report
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        != Some("article_lint_v3")
        || report.get("title").and_then(serde_json::Value::as_str) != Some(title)
    {
        bail!("mechanical article lint output does not identify the expected report and title");
    }
    let errors = report
        .get("errors")
        .and_then(serde_json::Value::as_u64)
        .context("mechanical article lint output omitted its error count")?;
    let expected_exit = if errors == 0 { Some(0) } else { Some(1) };
    if lint.exit_code != expected_exit {
        bail!("mechanical article lint exit code does not agree with its retained JSON report");
    }
    Ok(())
}

fn validate_stored_execution_root(
    execution_root: &Path,
    repository: &Path,
    run_directory: &Path,
) -> Result<()> {
    if !execution_root.is_absolute()
        || execution_root.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        bail!("execution workspace must be an absolute normalized path");
    }
    let execution = portable(execution_root).to_ascii_lowercase();
    for protected in [repository, run_directory] {
        let protected = portable(protected).to_ascii_lowercase();
        if execution == protected || execution.starts_with(&format!("{protected}/")) {
            bail!("execution workspace is inside the repository or retained run tree");
        }
    }
    Ok(())
}

fn verify_artifact(run_directory: &Path, artifact: &ArtifactIdentity) -> Result<()> {
    let path = resolve_output_path(run_directory, &artifact.locator)?;
    let (sha256, bytes) = sha256_file(&path)?;
    if sha256 != artifact.sha256 || bytes != artifact.bytes {
        bail!("artifact changed after receipt: {}", artifact.locator);
    }
    Ok(())
}

fn verify_output(run_directory: &Path, artifact: &OutputArtifact) -> Result<()> {
    let path = resolve_output_path(run_directory, &artifact.locator)?;
    let (sha256, bytes) = sha256_file(&path)?;
    if sha256 != artifact.stored_sha256 || bytes != artifact.stored_bytes {
        bail!("retained process output changed: {}", artifact.locator);
    }
    if artifact.observed_bytes < artifact.stored_bytes
        || artifact.truncated != (artifact.observed_bytes > artifact.stored_bytes)
        || (!artifact.truncated && artifact.sha256 != artifact.stored_sha256)
    {
        bail!(
            "retained process output metadata is inconsistent: {}",
            artifact.locator
        );
    }
    Ok(())
}

struct ParticipantExportEvidence<'a> {
    expected_stage: &'a str,
    retained_request: &'a ArtifactIdentity,
    export: &'a ParticipantExport,
    inputs: &'a [ArtifactIdentity],
    observations: &'a [MechanicalObservation],
    external_required: bool,
}

fn verify_participant_export(
    repository: &Path,
    run_directory: &Path,
    run_id: &str,
    evidence: ParticipantExportEvidence<'_>,
) -> Result<()> {
    let ParticipantExportEvidence {
        expected_stage,
        retained_request,
        export,
        inputs,
        observations,
        external_required,
    } = evidence;
    let expected_token = participant_export_token(expected_stage)?;
    if export.root != expected_token {
        bail!("participant export root token does not match its stage");
    }
    let repository = fs::canonicalize(repository)?;
    let run_directory = fs::canonicalize(run_directory)?;
    if !external_required {
        let retained_request_path = resolve_output_path(&run_directory, &retained_request.locator)?;
        let (sha256, bytes) = sha256_file(&retained_request_path)?;
        if export.request.sha256 != sha256 || export.request.bytes != bytes {
            bail!("archived participant export request identity differs from retained evidence");
        }
        if export.output_directory != "output" {
            bail!("participant export has an invalid output directory");
        }
        for input in inputs {
            verify_artifact(&run_directory, input)?;
        }
        verify_observations(&run_directory, observations)?;
        return Ok(());
    }
    let stored_root = operational_participant_export_root(run_id, export)?;
    validate_stored_export_root(&stored_root, &repository, &run_directory)?;
    if !stored_root.exists() {
        bail!(
            "participant export is required until its submission is retained: {}",
            stored_root.display()
        );
    }
    let export_root = fs::canonicalize(&stored_root)
        .with_context(|| format!("failed to resolve participant export root {}", export.root))?;
    if export_root.starts_with(&repository)
        || export_root.starts_with(&run_directory)
        || run_directory.starts_with(&export_root)
    {
        bail!("participant export is not physically isolated from repository and holdout roots");
    }
    if export.output_directory != "output"
        || !resolve_output_path(&export_root, &export.output_directory)?.is_dir()
    {
        bail!("participant export has no isolated output directory");
    }
    verify_artifact(&export_root, &export.request)?;
    let retained_request_path = resolve_output_path(&run_directory, &retained_request.locator)?;
    let exported_request_path = resolve_output_path(&export_root, &export.request.locator)?;
    if fs::read(retained_request_path)? != fs::read(exported_request_path)? {
        bail!("participant export request differs from the retained request");
    }

    let mut expected = BTreeSet::from([export.request.locator.clone()]);
    for input in inputs {
        verify_artifact(&export_root, input)?;
        expected.insert(input.locator.clone());
    }
    for observation in observations {
        verify_output(&export_root, &observation.stdout)?;
        verify_output(&export_root, &observation.stderr)?;
        expected.insert(observation.stdout.locator.clone());
        expected.insert(observation.stderr.locator.clone());
    }
    let mut observed = BTreeSet::new();
    for entry in WalkDir::new(&export_root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!(
                "participant export contains a symlink: {}",
                entry.path().display()
            );
        }
        if entry.file_type().is_file() {
            let locator = relative_locator(&export_root, entry.path())?;
            if !locator.starts_with("output/") {
                observed.insert(locator);
            }
        }
    }
    if observed != expected {
        bail!(
            "participant export file allowlist differs: expected {:?}, observed {:?}",
            expected,
            observed
        );
    }
    Ok(())
}

fn validate_stored_export_root(
    export_root: &Path,
    repository: &Path,
    run_directory: &Path,
) -> Result<()> {
    if !export_root.is_absolute()
        || export_root.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        bail!("participant export root must be an absolute normalized path");
    }
    let export = portable(export_root).to_ascii_lowercase();
    for protected in [repository, run_directory] {
        let protected = portable(protected).to_ascii_lowercase();
        if export == protected || export.starts_with(&format!("{protected}/")) {
            bail!("participant export root is inside a protected authority tree");
        }
    }
    Ok(())
}

fn artifact_identity(run_directory: &Path, path: &Path) -> Result<ArtifactIdentity> {
    let (sha256, bytes) = sha256_file(path)?;
    Ok(ArtifactIdentity {
        locator: relative_locator(run_directory, path)?,
        sha256,
        bytes,
    })
}

fn extension_for_locator(locator: &str) -> String {
    Path::new(locator)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default()
}

fn oracle_redacted_assignment_bytes(assignment: &ProseAssignment) -> Result<Vec<u8>> {
    let mut public_assignment = assignment.clone();
    public_assignment.oracle = None;
    let mut bytes = serde_json::to_vec_pretty(&public_assignment)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prose_model::{
        Participant, ParticipantKind, ReviewAxis, ReviewObservation, ReviewScope, SourceVerdict,
    };

    #[test]
    fn public_mechanical_bytes_hide_windows_verbatim_runner_paths() {
        let root =
            Path::new(r"\\?\C:\Users\Onno\AppData\Local\Temp\wikitest-execution-workspaces\run-1");
        let tool = Path::new(r"\\?\F:\AI\wiki\tools\wikitool\dist\release\wikitool.exe");
        let config = root.join(".wikitool/config.toml");
        let json_root = serde_json::to_string(&root.to_string_lossy()).expect("JSON path");
        let bytes = format!(
            "root={}\njson={json_root}\nconfig={}\ntool={}\n",
            root.display(),
            config.display(),
            tool.display()
        );

        let canonical = canonicalize_runner_owned_paths(bytes.as_bytes(), root, tool)
            .expect("canonical public output");
        let canonical = String::from_utf8(canonical).expect("UTF-8 output");

        assert!(canonical.contains(MECHANICAL_ROOT_TOKEN));
        assert!(canonical.contains(MECHANICAL_CONFIG_TOKEN));
        assert!(canonical.contains(TOOL_BINARY_TOKEN));
        assert!(!canonical.contains("Onno"));
        assert!(!canonical.contains(r"\\?\C:\"));
    }

    #[test]
    fn public_path_canonicalization_does_not_rewrite_unrelated_content() {
        let root = Path::new(r"\\?\C:\Users\Onno\AppData\Local\Temp\wikitest\run-1");
        let tool = Path::new(r"\\?\F:\AI\wiki\wikitool.exe");
        let unrelated = b"Source text mentions C:\\Users\\Onno\\Documents\\notes.txt verbatim.";

        let canonical = canonicalize_runner_owned_paths(unrelated, root, tool)
            .expect("canonical public output");

        assert_eq!(canonical, unrelated);
    }

    #[test]
    fn canonical_mechanical_argv_contains_only_stable_typed_paths() {
        let argv = canonical_mechanical_runtime_argv();
        let serialized = serde_json::to_string(&argv).expect("serialize argv");

        assert!(serialized.contains(MECHANICAL_ROOT_TOKEN));
        assert!(serialized.contains(MECHANICAL_DATA_TOKEN));
        assert!(serialized.contains(MECHANICAL_CONFIG_TOKEN));
        assert!(!serialized.contains("Users"));
        assert!(!serialized.contains("AppData"));
    }

    #[test]
    fn oracle_requires_declared_tags_and_disposition() {
        let oracle = crate::prose_model::ReviewOracle {
            required_finding_tags: vec!["host-framing".to_owned()],
            forbidden_finding_tags: Vec::new(),
            axis_expectations: vec![crate::prose_model::OracleAxisExpectation {
                axis: "reader-value".to_owned(),
                allowed_verdicts: vec![crate::prose_model::AxisVerdict::Fail],
            }],
            allowed_dispositions: vec![ReviewDisposition::Revise],
        };
        let submission = ReviewSubmission {
            schema: REVIEW_SUBMISSION_SCHEMA.to_owned(),
            run_id: "run-1".to_owned(),
            assignment_id: "assignment-1".to_owned(),
            review_packet_sha256: "a".repeat(64),
            candidate_sha256: Some("b".repeat(64)),
            finding_tag_vocabulary: vec!["host-framing".to_owned()],
            reviewer: Participant {
                id: "reviewer".to_owned(),
                display_name: "Reviewer".to_owned(),
                kind: ParticipantKind::Human,
                execution: None,
            },
            scope: ReviewScope::Complete,
            reader_verdict: "No.".to_owned(),
            source_verdict: SourceVerdict::Complete,
            disposition: ReviewDisposition::Revise,
            findings: Vec::new(),
            observations: vec![ReviewObservation {
                tag: "host-framing".to_owned(),
                evidence: "A positive observation is not a finding.".to_owned(),
            }],
            axes: vec![ReviewAxis {
                axis: "reader-value".to_owned(),
                verdict: crate::prose_model::AxisVerdict::Concern,
                rationale: "The lead is disproportionate.".to_owned(),
            }],
            residual_risk: "None.".to_owned(),
        };
        let result = evaluate_oracle(&oracle, &submission);
        assert!(!result.passed);
        assert_eq!(result.missing_required_tags, ["host-framing"]);
        assert_eq!(result.failed_axis_expectations, ["reader-value"]);
    }

    #[test]
    fn authoring_coverage_requires_an_accepted_demonstrated_article() {
        let complete_blocked = DemonstrationEvidence {
            mode: ProseMode::Authoring,
            receipt_status: ProseRunStatus::ReviewedBlock,
            oracle_passed: Some(Some(true)),
            review_complete: true,
            review_disposition: Some(ReviewDisposition::Block),
            author_complete: true,
        };
        assert_eq!(
            classify_prose_coverage(complete_blocked),
            ProseCoverageStatus::AuthoringRejected
        );
        assert_eq!(
            classify_prose_coverage(DemonstrationEvidence {
                oracle_passed: Some(None),
                ..complete_blocked
            }),
            ProseCoverageStatus::MissingOracle
        );
        assert_eq!(
            classify_prose_coverage(DemonstrationEvidence {
                receipt_status: ProseRunStatus::ReviewedAccept,
                review_disposition: Some(ReviewDisposition::Accept),
                author_complete: false,
                ..complete_blocked
            }),
            ProseCoverageStatus::AuthoringIncomplete
        );
        assert_eq!(
            classify_prose_coverage(DemonstrationEvidence {
                receipt_status: ProseRunStatus::ReviewedAccept,
                review_disposition: Some(ReviewDisposition::Accept),
                ..complete_blocked
            }),
            ProseCoverageStatus::Demonstrated
        );
    }

    #[test]
    fn participant_packet_digest_does_not_commit_to_the_hidden_oracle() {
        let first: ProseAssignment = serde_json::from_value(serde_json::json!({
            "schema": crate::prose_model::PROSE_ASSIGNMENT_SCHEMA,
            "id": "oracle-commitment",
            "title": "Oracle commitment",
            "description": "Regression fixture",
            "mode": "review",
            "complexity": "focused",
            "coverage": ["review"],
            "article": {
                "title": "Example",
                "namespace": "Main",
                "article_object": "Example object",
                "reader_need": "Example need",
                "sensitive": false
            },
            "review_instructions": [],
            "sources": [],
            "finding_tag_vocabulary": ["alpha", "beta"],
            "review_axes": ["reader-value"],
            "candidate": {
                "id": "candidate",
                "root": "assignment",
                "locator": "candidate.wiki",
                "sha256": "a".repeat(64)
            },
            "oracle": {
                "required_finding_tags": ["alpha"],
                "allowed_dispositions": ["revise"]
            }
        }))
        .expect("assignment");
        let mut second = first.clone();
        second
            .oracle
            .as_mut()
            .expect("oracle")
            .required_finding_tags = vec!["beta".to_owned()];

        let first_public = oracle_redacted_assignment_bytes(&first).expect("public assignment");
        let second_public = oracle_redacted_assignment_bytes(&second).expect("public assignment");
        assert_eq!(first_public, second_public);
        assert_ne!(
            sha256_bytes(&serde_json::to_vec(&first).expect("authority")),
            sha256_bytes(&serde_json::to_vec(&second).expect("authority"))
        );
        let packet_digest = |assignment: &ProseAssignment, public_bytes: &[u8]| {
            let packet = PacketBinding {
                schema: PROSE_PACKET_SCHEMA.to_owned(),
                assignment: ProseAssignmentIdentity {
                    id: assignment.id.clone(),
                    title: assignment.title.clone(),
                    mode: assignment.mode,
                    coverage: assignment.coverage.clone(),
                    locator: "inputs/assignment.json".to_owned(),
                    sha256: sha256_bytes(public_bytes),
                },
                inputs: Vec::new(),
            };
            sha256_bytes(&serde_json::to_vec(&packet).expect("packet"))
        };
        assert_eq!(
            packet_digest(&first, &first_public),
            packet_digest(&second, &second_public)
        );
    }

    #[test]
    fn reviewer_visible_inputs_exclude_author_only_material() {
        let artifact = |locator: &str| ArtifactIdentity {
            locator: locator.to_owned(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        let candidate = artifact("author/candidate.wiki");
        let visible = review_visible_inputs(
            &[
                artifact("inputs/assignment.json"),
                artifact("inputs/author-instructions/wiki-writing.md"),
                artifact("inputs/review-instructions/prose-review.md"),
                artifact("inputs/context/context.txt"),
                artifact("inputs/sources/source.txt"),
                artifact("author/submission-template.json"),
                artifact("author/claim-map.json"),
            ],
            Some(&candidate),
        );
        let locators = visible
            .iter()
            .map(|artifact| artifact.locator.as_str())
            .collect::<BTreeSet<_>>();
        assert!(!locators.contains("inputs/assignment.json"));
        assert!(!locators.contains("inputs/author-instructions/wiki-writing.md"));
        assert!(!locators.contains("author/submission-template.json"));
        assert!(locators.contains("inputs/review-instructions/prose-review.md"));
        assert!(locators.contains("author/candidate.wiki"));
        assert!(!locators.contains("author/claim-map.json"));
    }

    #[test]
    fn replay_rejects_an_author_only_input_added_to_the_review_packet() {
        let artifact = |locator: &str| ArtifactIdentity {
            locator: locator.to_owned(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        let expected = vec![artifact("inputs/review-instructions/prose-review.md")];
        let mut leaked = expected.clone();
        leaked.push(artifact("inputs/author-instructions/wiki-writing.md"));
        let error = require_exact_review_inputs(&leaked, &expected)
            .expect_err("author-only review input must fail replay");
        assert!(error.to_string().contains("reviewer allowlist"));
    }

    #[test]
    fn reviewer_packet_does_not_commit_to_internal_or_author_only_assignment_fields() {
        let first: ProseAssignment = serde_json::from_value(serde_json::json!({
            "schema": crate::prose_model::PROSE_ASSIGNMENT_SCHEMA,
            "id": "review-host-framing",
            "title": "Expected host framing failure",
            "description": "Internal operator description",
            "mode": "review",
            "complexity": "focused",
            "coverage": ["host-framing-review"],
            "article": {
                "title": "Example",
                "namespace": "Main",
                "article_object": "Example object",
                "reader_need": "Example need",
                "sensitive": false
            },
            "author_instructions": [],
            "review_instructions": [],
            "sources": [],
            "do_not_assert": ["Internal author constraint"],
            "allowed_decisions": ["article"],
            "finding_tag_vocabulary": ["due-weight", "source-entailment"],
            "review_axes": ["reader-value"],
            "candidate": {
                "id": "candidate",
                "root": "assignment",
                "locator": "candidate.wiki",
                "sha256": "a".repeat(64)
            },
            "claim_map": {
                "id": "author-rationale",
                "root": "assignment",
                "locator": "claim-map.json",
                "sha256": "b".repeat(64)
            },
            "oracle": {
                "required_finding_tags": ["due-weight"],
                "allowed_dispositions": ["revise"]
            }
        }))
        .expect("assignment");
        let mut second = first.clone();
        second.id = "review-sensitive-claim".to_owned();
        second.title = "Different internal expected result".to_owned();
        second.description = "Different internal operator description".to_owned();
        second.coverage = vec!["different-internal-coverage".to_owned()];
        second.author_instructions = vec![BoundInput {
            id: "private-author-procedure".to_owned(),
            root: crate::prose_model::InputRoot::Assignment,
            locator: "private-author.md".to_owned(),
            sha256: "c".repeat(64),
        }];
        second.do_not_assert = vec!["Different private author constraint".to_owned()];
        second.allowed_decisions = vec![crate::prose_model::ArticleDecision::Hold];
        second.claim_map = None;
        second.oracle = Some(crate::prose_model::ReviewOracle {
            required_finding_tags: vec!["source-entailment".to_owned()],
            forbidden_finding_tags: Vec::new(),
            axis_expectations: Vec::new(),
            allowed_dispositions: vec![ReviewDisposition::Block],
        });

        let participant_id = "case-deadbeef";
        assert_eq!(
            review_assignment_projection(participant_id, &first),
            review_assignment_projection(participant_id, &second)
        );
        let digest = |assignment: &ProseAssignment| {
            let binding = ReviewPacketBinding {
                schema: PROSE_PACKET_SCHEMA.to_owned(),
                assignment: review_assignment_projection(participant_id, assignment),
                inputs: vec![ArtifactIdentity {
                    locator: "author/candidate.wiki".to_owned(),
                    sha256: "a".repeat(64),
                    bytes: 1,
                }],
                mechanical_observations: Vec::new(),
            };
            let bytes = serde_json::to_vec(&binding).expect("review packet");
            let text = String::from_utf8(bytes.clone()).expect("UTF-8 packet");
            assert!(!text.contains("review-host-framing"));
            assert!(!text.contains("review-sensitive-claim"));
            assert!(!text.contains("Expected host framing failure"));
            sha256_bytes(&bytes)
        };
        assert_eq!(digest(&first), digest(&second));
        assert_ne!(
            oracle_redacted_assignment_bytes(&first).expect("author-visible assignment"),
            oracle_redacted_assignment_bytes(&second).expect("author-visible assignment")
        );
        assert_ne!(
            sha256_bytes(&serde_json::to_vec(&first).expect("first authority")),
            sha256_bytes(&serde_json::to_vec(&second).expect("second authority"))
        );
    }

    #[test]
    fn packet_inputs_match_exact_destinations_when_ids_share_a_prefix() {
        let artifact = |locator: &str, digest: char| ArtifactIdentity {
            locator: locator.to_owned(),
            sha256: digest.to_string().repeat(64),
            bytes: 1,
        };
        let bound_input = |id: &str| BoundInput {
            id: id.to_owned(),
            root: crate::prose_model::InputRoot::Assignment,
            locator: format!("{id}.md"),
            sha256: "a".repeat(64),
        };
        let artifacts = vec![
            artifact("inputs/review-instructions/source-long.md", 'a'),
            artifact("inputs/review-instructions/source.md", 'b'),
            artifact("inputs/sources/source-long.txt", 'c'),
            artifact("inputs/sources/source.txt", 'd'),
        ];
        let instructions = packet_inputs_for_prefix(
            &artifacts,
            "inputs/review-instructions",
            &[bound_input("source-long"), bound_input("source")],
        )
        .expect("exact instruction bindings");
        assert_eq!(instructions[0].artifact.sha256, "a".repeat(64));
        assert_eq!(instructions[1].artifact.sha256, "b".repeat(64));

        let source = |id: &str| SourceInput {
            id: id.to_owned(),
            title: id.to_owned(),
            locator: format!("{id}.txt"),
            sha256: "a".repeat(64),
            role: crate::prose_model::SourceRole::Fixture,
            citation: id.to_owned(),
        };
        let sources =
            packet_sources_from_receipt(&artifacts, &[source("source-long"), source("source")])
                .expect("exact source bindings");
        assert_eq!(sources[0].artifact.sha256, "c".repeat(64));
        assert_eq!(sources[1].artifact.sha256, "d".repeat(64));
    }

    #[test]
    fn canonical_skill_snapshots_preserve_relative_reference_links() {
        let repository = tempfile::tempdir().expect("repository");
        let run = tempfile::tempdir().expect("run");
        let skill_root = repository.path().join("agent-pack/skills/wiki-writing");
        fs::create_dir_all(skill_root.join("references")).expect("references");
        let skill_bytes = b"Read [evidence](references/evidence.md).\n";
        let reference_bytes = b"Evidence procedure.\n";
        atomic_write(&skill_root.join("SKILL.md"), skill_bytes).expect("skill");
        atomic_write(&skill_root.join("references/evidence.md"), reference_bytes)
            .expect("reference");
        let declarations = vec![
            BoundInput {
                id: "wiki-writing".to_owned(),
                root: crate::prose_model::InputRoot::Repository,
                locator: "agent-pack/skills/wiki-writing/SKILL.md".to_owned(),
                sha256: sha256_bytes(skill_bytes),
            },
            BoundInput {
                id: "evidence".to_owned(),
                root: crate::prose_model::InputRoot::Repository,
                locator: "agent-pack/skills/wiki-writing/references/evidence.md".to_owned(),
                sha256: sha256_bytes(reference_bytes),
            },
        ];
        let mut inputs = Vec::new();
        let packet = snapshot_bound_inputs(
            &declarations,
            repository.path(),
            repository.path(),
            run.path(),
            "inputs/author-instructions",
            &mut inputs,
        )
        .expect("skill snapshot");
        let retained_skill = run
            .path()
            .join("inputs/author-instructions/wiki-writing/SKILL.md");
        let linked_reference = retained_skill
            .parent()
            .expect("skill parent")
            .join("references/evidence.md");
        assert!(retained_skill.is_file());
        assert!(linked_reference.is_file());
        assert_eq!(
            packet[0].artifact.locator,
            "inputs/author-instructions/wiki-writing/SKILL.md"
        );
        assert_eq!(
            packet[1].artifact.locator,
            "inputs/author-instructions/wiki-writing/references/evidence.md"
        );
    }

    #[test]
    fn prose_suite_preserves_preparation_and_final_child_receipts_after_relocation() {
        let original = tempfile::tempdir().expect("original suite");
        let original_receipt = original.path().join("runs/child/receipt.json");
        let preparation_snapshot = original.path().join("prepared/child.receipt.json");
        atomic_write(&original_receipt, b"prepared child receipt\n").expect("child receipt");
        atomic_write(&preparation_snapshot, b"prepared child receipt\n")
            .expect("preparation snapshot");
        let mut run = ProseSuiteRun {
            assignment_id: "child".to_owned(),
            run_locator: "runs/child".to_owned(),
            preparation_receipt: artifact_identity(original.path(), &preparation_snapshot)
                .expect("preparation identity"),
            current_receipt: artifact_identity(original.path(), &original_receipt)
                .expect("current identity"),
            status: ProseRunStatus::AwaitingReview,
            coverage_status: ProseCoverageStatus::AwaitingReview,
        };
        assert!(!Path::new(&run.run_locator).is_absolute());
        assert_ne!(run.preparation_receipt.locator, run.current_receipt.locator);

        atomic_write(&original_receipt, b"evaluated child receipt\n")
            .expect("progress child receipt");
        run.current_receipt =
            artifact_identity(original.path(), &original_receipt).expect("final identity");
        verify_artifact(original.path(), &run.preparation_receipt)
            .expect("immutable preparation snapshot remains valid");

        let relocated = tempfile::tempdir().expect("relocated suite");
        let relocated_receipt = relocated.path().join(&run.current_receipt.locator);
        atomic_write(&relocated_receipt, b"evaluated child receipt\n").expect("relocated receipt");
        let relocated_preparation = relocated.path().join(&run.preparation_receipt.locator);
        atomic_write(&relocated_preparation, b"prepared child receipt\n")
            .expect("relocated preparation");
        assert_eq!(
            resolve_prose_suite_child_receipt(relocated.path(), &run).expect("relocatable child"),
            relocated_receipt
        );
        verify_artifact(relocated.path(), &run.current_receipt)
            .expect("final child identity survives relocation");
        verify_artifact(relocated.path(), &run.preparation_receipt)
            .expect("preparation identity survives relocation");
    }

    #[test]
    fn prose_suite_catalog_locator_never_embeds_an_absolute_host_root() {
        let source = Path::new(r"F:\AI\wiki\wikitest\prose\prose-suite.json");
        let locator = prose_suite_catalog_locator("remilia-prose-dogfood");
        assert_eq!(
            locator,
            "wikitest:catalog:prose-suite:remilia-prose-dogfood"
        );
        assert!(!locator.contains(&portable(source)));
        assert!(!locator.contains("F:/"));
        assert!(!Path::new(&locator).is_absolute());
    }

    #[test]
    fn participant_export_is_outside_repository_and_cannot_reach_holdout_by_parent_path() {
        let repository = tempfile::tempdir().expect("repository");
        let run_directory = repository.path().join("run");
        fs::create_dir_all(run_directory.join("review")).expect("review directory");
        fs::create_dir_all(run_directory.join("inputs")).expect("input directory");
        fs::create_dir_all(run_directory.join("holdout")).expect("holdout directory");
        let request_path = run_directory.join("review/request.json");
        atomic_write(&request_path, b"{}\n").expect("request");
        let assignment_path = run_directory.join("inputs/assignment.json");
        atomic_write(&assignment_path, b"{}\n").expect("assignment");
        atomic_write(
            &run_directory.join("holdout/assignment.json"),
            b"held-out oracle\n",
        )
        .expect("holdout");
        let input = artifact_identity(&run_directory, &assignment_path).expect("input identity");
        let retained_request =
            artifact_identity(&run_directory, &request_path).expect("request identity");
        let run_id = format!(
            "export-isolation-{}-{}",
            std::process::id(),
            unix_ms().expect("clock")
        );
        let export = write_participant_export(
            &run_id,
            "reviewer",
            repository.path(),
            &run_directory,
            &request_path,
            std::slice::from_ref(&input),
            &[],
        )
        .expect("participant export");
        assert_eq!(export.root, "wikitest:participant-export:reviewer");
        let export_root = operational_participant_export_root(&run_id, &export)
            .expect("operational participant export root");
        assert!(!export_root.starts_with(repository.path()));
        assert!(!export_root.join("../../holdout/assignment.json").exists());
        atomic_write(&export_root.join("output/review-submission.json"), b"{}\n")
            .expect("participant output");
        verify_participant_export(
            repository.path(),
            &run_directory,
            &run_id,
            ParticipantExportEvidence {
                expected_stage: "reviewer",
                retained_request: &retained_request,
                export: &export,
                inputs: std::slice::from_ref(&input),
                observations: &[],
                external_required: true,
            },
        )
        .expect("isolated export replay");
        fs::remove_dir_all(export_root).expect("remove participant export");
        verify_participant_export(
            repository.path(),
            &run_directory,
            &run_id,
            ParticipantExportEvidence {
                expected_stage: "reviewer",
                retained_request: &retained_request,
                export: &export,
                inputs: std::slice::from_ref(&input),
                observations: &[],
                external_required: false,
            },
        )
        .expect("archived export replay uses retained request evidence");
    }

    #[test]
    fn author_identity_cannot_review_its_own_run() {
        let participant = Participant {
            id: "same-agent".to_owned(),
            display_name: "Same agent".to_owned(),
            kind: ParticipantKind::Agent,
            execution: Some(crate::prose_model::AgentExecution {
                provider: "fixture-provider".to_owned(),
                model: "fixture-model".to_owned(),
                harness: "fixture-harness".to_owned(),
                harness_version: "1.0.0".to_owned(),
                invocation_id: "fixture-invocation".to_owned(),
                reasoning_effort: None,
                access: crate::prose_model::AgentAccess {
                    network: false,
                    ambient_repository: false,
                    tools: Vec::new(),
                },
                metrics: None,
            }),
        };
        let error = ensure_independent_reviewer(Some(&participant), &participant)
            .expect_err("self-review must be refused");
        assert!(error.to_string().contains("recorded author"));
    }
}
