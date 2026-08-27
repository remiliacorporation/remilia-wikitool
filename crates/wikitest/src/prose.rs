use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::artifact::{
    atomic_write, atomic_write_json, portable, relative_locator, resolve_existing_plain_file,
    resolve_output_path, sha256_bytes, sha256_file, unix_ms,
};
use crate::catalog::{Manifest, load_manifest, resolve_manifest};
use crate::model::{ArtifactIdentity, OutputArtifact, ToolIdentity};
use crate::process::{CapturedStream, probe_version, run_bounded};
use crate::prose_model::{
    AUTHOR_REQUEST_SCHEMA, AUTHOR_SUBMISSION_SCHEMA, AuthorRequest, AuthorStageReceipt,
    AuthorSubmission, AxisVerdict, BoundInput, CLAIM_MAP_SCHEMA, ClaimMap, MechanicalObservation,
    OracleEvaluation, PROSE_PACKET_SCHEMA, PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA,
    PacketBinding, PacketInput, PacketSource, ProseAssignment, ProseAssignmentIdentity, ProseMode,
    ProseReceipt, ProseRunStatus, ProseSuite, ProseSuiteIdentity, ProseSuiteReceipt, ProseSuiteRun,
    REVIEW_REQUEST_SCHEMA, REVIEW_SUBMISSION_SCHEMA, ReviewDisposition, ReviewPacketBinding,
    ReviewRequest, ReviewStageReceipt, ReviewSubmission, SourceInput,
};

const PROSE_PROCESS_TIMEOUT: Duration = Duration::from_secs(120);

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
    claim_map: Option<ArtifactIdentity>,
    mechanical_observations: Vec<MechanicalObservation>,
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
    let run_id = format!(
        "prose-{}-{created_at}-{}",
        assignment.id,
        std::process::id()
    );
    let run_directory = options.artifacts_root.join(&run_id);
    if run_directory.exists() {
        bail!("prose run already exists: {}", run_directory.display());
    }
    fs::create_dir_all(run_directory.join("inputs"))?;
    fs::create_dir_all(run_directory.join("holdout"))?;
    fs::create_dir_all(run_directory.join("tool"))?;

    let assignment_sha256 = sha256_bytes(&assignment_bytes);
    let authority_path = run_directory.join("holdout/assignment.json");
    atomic_write(&authority_path, &assignment_bytes)?;
    let authority = artifact_identity(&run_directory, &authority_path)?;
    let mut public_assignment = assignment.clone();
    public_assignment.oracle = None;
    let mut public_assignment_bytes = serde_json::to_vec_pretty(&public_assignment)?;
    public_assignment_bytes.push(b'\n');
    let assignment_path = run_directory.join("inputs/assignment.json");
    atomic_write(&assignment_path, &public_assignment_bytes)?;
    let assignment_artifact = artifact_identity(&run_directory, &assignment_path)?;
    let assignment_identity = ProseAssignmentIdentity {
        id: assignment.id.clone(),
        title: assignment.title.clone(),
        mode: assignment.mode,
        coverage: assignment.coverage.clone(),
        locator: path
            .strip_prefix(&options.repository)
            .map(portable)
            .unwrap_or_else(|_| portable(&path)),
        sha256: assignment_sha256,
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
        let templates = write_author_templates(&run_id, &assignment, &run_directory)?;
        inputs.push(templates.0.clone());
        inputs.push(templates.1.clone());
        Some(templates)
    } else {
        None
    };

    let packet_binding = PacketBinding {
        schema: PROSE_PACKET_SCHEMA.to_owned(),
        assignment: assignment_identity.clone(),
        inputs: inputs.clone(),
    };
    let packet_path = run_directory.join("inputs/packet.json");
    atomic_write_json(&packet_path, &packet_binding)?;
    let packet = artifact_identity(&run_directory, &packet_path)?;
    let tool = tool_identity(options, &run_directory)?;

    let mut author_request = None;
    let mut review_request = None;
    let mut review_packet = None;
    let status = match assignment.mode {
        ProseMode::Authoring => {
            let (submission_template, claim_map_template) = author_templates
                .context("authoring assignment has no generated submission templates")?;
            let request = AuthorRequest {
                schema: AUTHOR_REQUEST_SCHEMA.to_owned(),
                run_id: run_id.clone(),
                assignment_id: assignment.id.clone(),
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
            let generated = build_review_request(
                &run_id,
                &assignment,
                &assignment_identity,
                &inputs,
                ReviewRequestParts {
                    instructions: review_instructions,
                    context,
                    sources,
                    candidate: Some(candidate.clone()),
                    claim_map: fixture_claim_map,
                    mechanical_observations: mechanical,
                },
                &run_directory,
            )?;
            review_packet = Some(generated.0);
            review_request = Some(generated.1);
            ProseRunStatus::AwaitingReview
        }
    };

    let receipt = ProseReceipt {
        schema: PROSE_RECEIPT_SCHEMA.to_owned(),
        run_id,
        assignment: assignment_identity,
        tool,
        status,
        created_at_unix_ms: created_at,
        updated_at_unix_ms: created_at,
        evaluation_complete: false,
        authority,
        packet,
        inputs,
        author_request,
        author: None,
        review_request,
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
    let observed_coverage = prose_suite_coverage(&suite, &options.catalogs)?;
    let required = suite
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if !required.is_subset(&observed_coverage) {
        let missing = required
            .difference(&observed_coverage)
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
    let suite_input = run_directory.join("inputs/prose-suite.json");
    atomic_write(&suite_input, &suite_bytes)?;
    let (suite_sha256, _) = sha256_file(&suite_input)?;
    let suite_identity = ProseSuiteIdentity {
        id: suite.id.clone(),
        title: suite.title.clone(),
        locator: path
            .strip_prefix(&options.repository)
            .map(portable)
            .unwrap_or_else(|_| portable(&path)),
        sha256: suite_sha256,
    };

    let mut runs = Vec::new();
    for assignment_id in &suite.assignments {
        let assignment_path =
            resolve_manifest(assignment_id, &options.catalogs, "prose_assignment")?;
        let prepared = prepare_assignment(&assignment_path, options)?;
        let snapshot_path = run_directory
            .join("prepared")
            .join(format!("{assignment_id}.receipt.json"));
        let bytes = fs::read(&prepared.receipt_path)?;
        atomic_write(&snapshot_path, &bytes)?;
        runs.push(ProseSuiteRun {
            assignment_id: assignment_id.clone(),
            run_locator: portable(
                prepared
                    .receipt_path
                    .parent()
                    .context("prepared prose receipt has no parent")?,
            ),
            preparation_receipt: artifact_identity(&run_directory, &snapshot_path)?,
            status: prepared.receipt.status,
        });
    }
    let receipt = ProseSuiteReceipt {
        schema: PROSE_SUITE_RECEIPT_SCHEMA.to_owned(),
        run_id,
        suite: suite_identity,
        created_at_unix_ms: created_at,
        required_coverage: suite.required_coverage,
        observed_coverage: observed_coverage.into_iter().collect(),
        runs,
    };
    let receipt_path = run_directory.join("receipt.json");
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProseSuite {
        receipt,
        receipt_path,
    })
}

pub fn submit_author(
    run: &Path,
    submission_path: &Path,
    options: &ProseOptions,
) -> Result<PreparedProse> {
    let (mut receipt, receipt_path, run_directory, assignment) = load_prose_run(run)?;
    verify_current_receipt(&run_directory, &receipt)?;
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
        || submission.assignment_id != assignment.id
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
    let mut review_inputs = receipt.inputs.clone();
    if let Some(candidate) = &candidate {
        review_inputs.push(candidate.clone());
    }
    if let Some(claim_map) = &claim_map_artifact {
        review_inputs.push(claim_map.clone());
    }
    let generated = build_review_request(
        &receipt.run_id,
        &assignment,
        &receipt.assignment,
        &review_inputs,
        ReviewRequestParts {
            instructions: review_instructions,
            context,
            sources,
            candidate: candidate.clone(),
            claim_map: claim_map_artifact.clone(),
            mechanical_observations: mechanical.clone(),
        },
        &run_directory,
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
    receipt.review_packet = Some(generated.0);
    receipt.review_request = Some(generated.1);
    receipt.status = ProseRunStatus::AwaitingReview;
    receipt.updated_at_unix_ms = unix_ms()?;
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(PreparedProse {
        receipt,
        receipt_path,
    })
}

pub fn submit_review(run: &Path, submission_path: &Path) -> Result<PreparedProse> {
    let (mut receipt, receipt_path, run_directory, _public_assignment) = load_prose_run(run)?;
    let assignment = load_authority_assignment(&receipt, &run_directory)?;
    verify_current_receipt(&run_directory, &receipt)?;
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
        || submission.assignment_id != assignment.id
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
    assignment: &ProseAssignment,
    identity: &ProseAssignmentIdentity,
    review_inputs: &[ArtifactIdentity],
    parts: ReviewRequestParts,
    run_directory: &Path,
) -> Result<(ArtifactIdentity, ArtifactIdentity)> {
    let submission_template =
        write_review_template(run_id, assignment, parts.candidate.as_ref(), run_directory)?;
    let mut bound_inputs = review_inputs.to_vec();
    bound_inputs.push(submission_template.clone());
    let binding = ReviewPacketBinding {
        schema: PROSE_PACKET_SCHEMA.to_owned(),
        assignment: identity.clone(),
        inputs: bound_inputs,
        mechanical_observations: parts.mechanical_observations.clone(),
    };
    let binding_path = run_directory.join("review/packet.json");
    atomic_write_json(&binding_path, &binding)?;
    let binding_artifact = artifact_identity(run_directory, &binding_path)?;
    let request = ReviewRequest {
        schema: REVIEW_REQUEST_SCHEMA.to_owned(),
        run_id: run_id.to_owned(),
        assignment_id: assignment.id.clone(),
        review_packet_sha256: binding_artifact.sha256.clone(),
        artifact_root: ".".to_owned(),
        article: assignment.article.clone(),
        instructions: parts.instructions,
        context: parts.context,
        sources: parts.sources,
        candidate: parts.candidate,
        claim_map: parts.claim_map,
        mechanical_observations: parts.mechanical_observations,
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
    let export_root = run_directory.join("review/export");
    for input in &binding.inputs {
        copy_artifact_to_export(run_directory, &export_root, input)?;
    }
    for observation in &request.mechanical_observations {
        copy_output_to_export(run_directory, &export_root, &observation.stdout)?;
        copy_output_to_export(run_directory, &export_root, &observation.stderr)?;
    }
    let request_path = export_root.join("request.json");
    atomic_write_json(&request_path, &request)?;
    let request_artifact = artifact_identity(run_directory, &request_path)?;
    Ok((binding_artifact, request_artifact))
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

fn write_author_templates(
    run_id: &str,
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
        "assignment_id": assignment.id,
        "packet_sha256": "replace-with-packet-sha256-from-request",
        "author": {
            "id": "replace-with-stable-participant-id",
            "display_name": "Replace with author name",
            "kind": "agent",
            "provider": "Replace or remove",
            "model": "Replace or remove",
            "invocation_id": "Replace or remove"
        },
        "decision": decision,
        "notes": "Describe the research and drafting decision.",
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
        "assignment_id": assignment.id,
        "review_packet_sha256": "replace-with-review-packet-sha256-from-request",
        "candidate_sha256": candidate.map(|artifact| artifact.sha256.as_str()),
        "reviewer": {
            "id": "replace-with-stable-participant-id",
            "display_name": "Replace with reviewer name",
            "kind": "agent",
            "provider": "Replace or remove",
            "model": "Replace or remove",
            "invocation_id": "Replace or remove"
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
            let destination = format!(
                "{destination_root}/{}{}",
                input.id,
                extension_for_locator(&input.locator)
            );
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
            let artifact = artifacts
                .iter()
                .find(|artifact| {
                    artifact.locator.starts_with(prefix)
                        && artifact
                            .locator
                            .rsplit('/')
                            .next()
                            .is_some_and(|name| name.starts_with(&input.id))
                })
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
            let artifact = artifacts
                .iter()
                .find(|artifact| {
                    artifact.locator.starts_with("inputs/sources/")
                        && artifact
                            .locator
                            .rsplit('/')
                            .next()
                            .is_some_and(|name| name.starts_with(&source.id))
                })
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

fn run_mechanical_observations(
    candidate: &ArtifactIdentity,
    title: &str,
    run_directory: &Path,
    options: &ProseOptions,
) -> Result<Vec<MechanicalObservation>> {
    let project = run_directory.join("mechanical/project");
    fs::create_dir_all(&project)?;
    let candidate_path = resolve_output_path(run_directory, &candidate.locator)?;
    let candidate_bytes = fs::read(candidate_path)?;
    let init_argv = vec![
        "--project-root".to_owned(),
        project.to_string_lossy().into_owned(),
        "init".to_owned(),
        "--no-network".to_owned(),
    ];
    let init = run_observation("init", &init_argv, &project, run_directory, options)?;
    let mut observations = vec![init];
    if observations[0].exit_code == Some(0) && !observations[0].timed_out {
        let draft_path = project.join(".wikitool/drafts/candidate.wiki");
        atomic_write(&draft_path, &candidate_bytes)?;
        let lint_argv = vec![
            "--project-root".to_owned(),
            project.to_string_lossy().into_owned(),
            "article".to_owned(),
            "lint".to_owned(),
            ".wikitool/drafts/candidate.wiki".to_owned(),
            "--title".to_owned(),
            title.to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ];
        observations.push(run_observation(
            "lint",
            &lint_argv,
            &project,
            run_directory,
            options,
        )?);
    }
    Ok(observations)
}

fn run_observation(
    id: &str,
    argv: &[String],
    cwd: &Path,
    run_directory: &Path,
    options: &ProseOptions,
) -> Result<MechanicalObservation> {
    let stdout_path = run_directory.join(format!("mechanical/{id}.stdout.txt"));
    let stderr_path = run_directory.join(format!("mechanical/{id}.stderr.txt"));
    let outcome = run_bounded(
        &options.wikitool,
        argv,
        cwd,
        &BTreeMap::new(),
        PROSE_PROCESS_TIMEOUT,
        options.maximum_output_bytes,
        &stdout_path,
        &stderr_path,
    )?;
    Ok(MechanicalObservation {
        argv: argv.to_vec(),
        exit_code: outcome.status.code(),
        timed_out: outcome.timed_out,
        duration_ms: outcome.duration_ms,
        stdout: output_artifact(run_directory, &stdout_path, &outcome.stdout)?,
        stderr: output_artifact(run_directory, &stderr_path, &outcome.stderr)?,
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
        locator: options
            .wikitool
            .strip_prefix(&options.repository)
            .map(portable)
            .unwrap_or_else(|_| portable(&options.wikitool)),
        sha256,
        version,
    })
}

fn output_artifact(
    run_directory: &Path,
    path: &Path,
    stream: &CapturedStream,
) -> Result<OutputArtifact> {
    Ok(OutputArtifact {
        locator: relative_locator(run_directory, path)?,
        sha256: stream.sha256.clone(),
        stored_sha256: stream.stored_sha256.clone(),
        observed_bytes: stream.observed_bytes,
        stored_bytes: stream.stored_bytes,
        truncated: stream.truncated,
    })
}

pub(crate) fn evaluate_oracle(
    oracle: &crate::prose_model::ReviewOracle,
    submission: &ReviewSubmission,
) -> OracleEvaluation {
    let tags = submission
        .findings
        .iter()
        .flat_map(|finding| finding.tags.iter().cloned())
        .chain(
            submission
                .observations
                .iter()
                .map(|observation| observation.tag.clone()),
        )
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
    if receipt.authority.sha256 != receipt.assignment.sha256 {
        bail!("retained prose authority does not match the assignment identity");
    }
    let path = resolve_output_path(run_directory, &receipt.authority.locator)?;
    let assignment: ProseAssignment = serde_json::from_slice(&fs::read(&path)?)
        .context("invalid retained assignment authority")?;
    assignment.validate()?;
    if assignment.id != receipt.assignment.id
        || assignment.title != receipt.assignment.title
        || assignment.mode != receipt.assignment.mode
        || assignment.coverage != receipt.assignment.coverage
    {
        bail!("prose assignment authority identity does not match the receipt");
    }
    Ok(assignment)
}

fn verify_current_receipt(run_directory: &Path, receipt: &ProseReceipt) -> Result<()> {
    verify_artifact(run_directory, &receipt.authority)?;
    verify_artifact(run_directory, &receipt.packet)?;
    for input in &receipt.inputs {
        verify_artifact(run_directory, input)?;
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
    Ok(())
}

fn verify_observations(run_directory: &Path, values: &[MechanicalObservation]) -> Result<()> {
    for observation in values {
        verify_output(run_directory, &observation.stdout)?;
        verify_output(run_directory, &observation.stderr)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prose_model::{
        Participant, ParticipantKind, ReviewAxis, ReviewScope, SourceVerdict,
    };

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
            reviewer: Participant {
                id: "reviewer".to_owned(),
                display_name: "Reviewer".to_owned(),
                kind: ParticipantKind::Human,
                provider: None,
                model: None,
                invocation_id: None,
            },
            scope: ReviewScope::Complete,
            reader_verdict: "No.".to_owned(),
            source_verdict: SourceVerdict::Complete,
            disposition: ReviewDisposition::Revise,
            findings: Vec::new(),
            observations: Vec::new(),
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
    fn author_identity_cannot_review_its_own_run() {
        let participant = Participant {
            id: "same-agent".to_owned(),
            display_name: "Same agent".to_owned(),
            kind: ParticipantKind::Agent,
            provider: None,
            model: None,
            invocation_id: None,
        };
        let error = ensure_independent_reviewer(Some(&participant), &participant)
            .expect_err("self-review must be refused");
        assert!(error.to_string().contains("recorded author"));
    }
}
