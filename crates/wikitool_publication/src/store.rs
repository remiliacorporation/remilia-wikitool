use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::PublicationWorkspace;
use crate::acceptance::{
    ACCEPTANCE_DECISION, ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION, ArticleAcceptanceLedgerEntry,
    ArticlePublicationAuthority, EDITOR_IDENTITY_ASSURANCE, normalize_relative_path,
    validate_target_relative_path,
};
use crate::support::{compute_sha256, normalize_path};

const ACCEPTANCE_STORE_SCHEMA_VERSION: &str = "acceptance_store_v1";
const STORE_SCHEMA_KEY: &str = "schema_version";

const ACCEPTANCE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS acceptance_decisions (
    decision_id TEXT PRIMARY KEY,
    decision_kind TEXT NOT NULL,
    changeset_sha256 TEXT,
    accepted_at_unix INTEGER NOT NULL,
    target_api_url TEXT NOT NULL,
    site_adapter_id TEXT NOT NULL,
    publication_policy_sha256 TEXT NOT NULL,
    receipt_sha256 TEXT NOT NULL,
    receipt_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS article_acceptance_authorizations (
    target_identity TEXT PRIMARY KEY,
    decision_id TEXT NOT NULL,
    title TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    target_relative_path TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    accepted_at_unix INTEGER NOT NULL,
    target_api_url TEXT NOT NULL,
    site_adapter_id TEXT NOT NULL,
    publication_policy_sha256 TEXT NOT NULL,
    ledger_sha256 TEXT NOT NULL,
    ledger_json TEXT NOT NULL,
    FOREIGN KEY (decision_id) REFERENCES acceptance_decisions(decision_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_article_acceptance_decision
    ON article_acceptance_authorizations(decision_id);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcceptanceDecisionKind {
    SingleArticle,
    ArticleChangeset,
}

impl AcceptanceDecisionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SingleArticle => "single_article",
            Self::ArticleChangeset => "article_changeset",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "single_article" => Ok(Self::SingleArticle),
            "article_changeset" => Ok(Self::ArticleChangeset),
            _ => bail!("acceptance store contains unsupported decision kind {value:?}"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AcceptanceStoreDecision {
    pub decision_id: String,
    pub kind: AcceptanceDecisionKind,
    pub changeset_sha256: Option<String>,
    pub human_editor_claim: String,
    pub editor_identity_assurance: String,
    pub decision: String,
    pub accepted_at_unix: u64,
    pub publication_authority: ArticlePublicationAuthority,
    pub receipt_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredAcceptanceDecision {
    pub decision_id: String,
    pub kind: AcceptanceDecisionKind,
    pub changeset_sha256: Option<String>,
    pub accepted_at_unix: u64,
    pub publication_authority: ArticlePublicationAuthority,
    pub receipt_sha256: String,
    pub receipt_json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedArticleAuthorization {
    pub ledger_entry: ArticleAcceptanceLedgerEntry,
    pub decision: StoredAcceptanceDecision,
}

#[derive(Debug)]
struct StoredAuthorizationRow {
    decision_id: String,
    title: String,
    source_relative_path: String,
    target_relative_path: String,
    content_sha256: String,
    accepted_at_unix: u64,
    publication_authority: ArticlePublicationAuthority,
    ledger_sha256: String,
    ledger_json: String,
}

pub(crate) fn commit_single_article_acceptance(
    paths: &PublicationWorkspace,
    ledger_entry: &ArticleAcceptanceLedgerEntry,
) -> Result<PathBuf> {
    let receipt_json = serde_json::to_string(ledger_entry)
        .context("failed to encode single-article acceptance decision")?;
    let decision_id = compute_sha256(&format!(
        "article_acceptance_single_decision_v1\n{receipt_json}"
    ));
    let publication_authority = ledger_entry
        .publication_authority
        .clone()
        .context("single-article acceptance is missing publication authority")?;
    let decision = AcceptanceStoreDecision {
        decision_id,
        kind: AcceptanceDecisionKind::SingleArticle,
        changeset_sha256: None,
        human_editor_claim: ledger_entry.human_editor_claim.clone(),
        editor_identity_assurance: ledger_entry.editor_identity_assurance.clone(),
        decision: ledger_entry.decision.clone(),
        accepted_at_unix: ledger_entry.accepted_at_unix,
        publication_authority,
        receipt_json,
    };
    commit_acceptance_transaction(paths, &decision, std::slice::from_ref(ledger_entry))
}

pub(crate) fn commit_acceptance_transaction(
    paths: &PublicationWorkspace,
    decision: &AcceptanceStoreDecision,
    ledger_entries: &[ArticleAcceptanceLedgerEntry],
) -> Result<PathBuf> {
    commit_acceptance_transaction_inner(paths, decision, ledger_entries, None)
}

#[cfg(test)]
pub(crate) fn commit_acceptance_transaction_with_fault(
    paths: &PublicationWorkspace,
    decision: &AcceptanceStoreDecision,
    ledger_entries: &[ArticleAcceptanceLedgerEntry],
    fail_after_authorizations: usize,
) -> Result<PathBuf> {
    commit_acceptance_transaction_inner(
        paths,
        decision,
        ledger_entries,
        Some(fail_after_authorizations),
    )
}

fn commit_acceptance_transaction_inner(
    paths: &PublicationWorkspace,
    decision: &AcceptanceStoreDecision,
    ledger_entries: &[ArticleAcceptanceLedgerEntry],
    fail_after_authorizations: Option<usize>,
) -> Result<PathBuf> {
    validate_commit_inputs(decision, ledger_entries)?;
    let mut connection = open_or_create_acceptance_store(paths)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("failed to begin publication-acceptance transaction")?;
    insert_decision(&transaction, decision)?;
    for (index, ledger_entry) in ledger_entries.iter().enumerate() {
        upsert_authorization(&transaction, decision, ledger_entry)?;
        if fail_after_authorizations == Some(index + 1) {
            bail!(
                "injected publication-acceptance failure after {} authorization(s)",
                index + 1
            );
        }
    }
    transaction
        .commit()
        .context("failed to commit publication-acceptance transaction")?;
    Ok(paths.acceptance_store_path())
}

pub(crate) fn load_article_authorization(
    paths: &PublicationWorkspace,
    target_relative_path: &str,
) -> Result<LoadedArticleAuthorization> {
    validate_target_relative_path(target_relative_path)?;
    let store_path = paths.acceptance_store_path();
    if !store_path.is_file() {
        return missing_authorization(paths, target_relative_path, &store_path);
    }
    let connection = open_existing_acceptance_store(paths)?;
    let target_identity = portable_target_identity(target_relative_path);
    let row = connection
        .query_row(
            "SELECT
                decision_id,
                title,
                source_relative_path,
                target_relative_path,
                content_sha256,
                accepted_at_unix,
                target_api_url,
                site_adapter_id,
                publication_policy_sha256,
                ledger_sha256,
                ledger_json
             FROM article_acceptance_authorizations
             WHERE target_identity = ?1",
            params![target_identity],
            decode_authorization_row,
        )
        .optional()
        .with_context(|| {
            format!(
                "failed to read publication acceptance from {}",
                store_path.display()
            )
        })?;
    let Some(row) = row else {
        return missing_authorization(paths, target_relative_path, &store_path);
    };
    let ledger_entry = validate_authorization_row(&row)?;
    let stored_decision = load_decision_from_connection(&connection, &row.decision_id)?;
    if stored_decision.accepted_at_unix != ledger_entry.accepted_at_unix
        || stored_decision.publication_authority
            != ledger_entry
                .publication_authority
                .clone()
                .context("acceptance authorization is missing publication authority")?
    {
        bail!("publication-acceptance authorization does not match its transactional decision");
    }
    match stored_decision.kind {
        AcceptanceDecisionKind::SingleArticle => {
            if ledger_entry.changeset_decision.is_some()
                || stored_decision.changeset_sha256.is_some()
                || stored_decision.receipt_json != row.ledger_json
            {
                bail!("single-article acceptance has an inconsistent transactional decision");
            }
        }
        AcceptanceDecisionKind::ArticleChangeset => {
            let binding = ledger_entry
                .changeset_decision
                .as_ref()
                .context("changeset acceptance is missing its transactional decision binding")?;
            if binding.decision_id != stored_decision.decision_id
                || Some(binding.changeset_sha256.as_str())
                    != stored_decision.changeset_sha256.as_deref()
            {
                bail!("changeset acceptance does not match its transactional decision");
            }
        }
    }
    Ok(LoadedArticleAuthorization {
        ledger_entry,
        decision: stored_decision,
    })
}

pub(crate) fn load_acceptance_decision(
    paths: &PublicationWorkspace,
    decision_id: &str,
) -> Result<StoredAcceptanceDecision> {
    let connection = open_existing_acceptance_store(paths)?;
    load_decision_from_connection(&connection, decision_id)
}

fn missing_authorization<T>(
    paths: &PublicationWorkspace,
    target_relative_path: &str,
    store_path: &Path,
) -> Result<T> {
    let legacy_path = legacy_json_ledger_path(paths, target_relative_path)?;
    if legacy_path.is_file() {
        bail!(
            "legacy JSON acceptance ledger exists at {} but is historical, unbound, and non-authoritative; repeat named-human review with `wikitool article accept` to create transactional authority in {}",
            normalize_path(&legacy_path),
            normalize_path(store_path)
        );
    }
    bail!(
        "transactional acceptance authorization is missing for {} in {}; run `wikitool article accept` for the exact prose",
        normalize_relative_path(target_relative_path),
        normalize_path(store_path)
    )
}

fn open_or_create_acceptance_store(paths: &PublicationWorkspace) -> Result<Connection> {
    let store_path = paths.acceptance_store_path();
    let parent = store_path
        .parent()
        .context("acceptance store path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let connection = Connection::open(&store_path)
        .with_context(|| format!("failed to open acceptance store {}", store_path.display()))?;
    configure_connection(&connection)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS acceptance_store_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .context("failed to initialize acceptance-store metadata")?;
    let existing_schema = connection
        .query_row(
            "SELECT value FROM acceptance_store_meta WHERE key = ?1",
            params![STORE_SCHEMA_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to inspect acceptance-store schema version")?;
    match existing_schema {
        Some(schema) if schema != ACCEPTANCE_STORE_SCHEMA_VERSION => bail!(
            "acceptance store {} uses incompatible schema {schema:?}; expected {ACCEPTANCE_STORE_SCHEMA_VERSION}",
            normalize_path(&store_path)
        ),
        Some(_) => {}
        None => {
            connection
                .execute(
                    "INSERT INTO acceptance_store_meta (key, value) VALUES (?1, ?2)",
                    params![STORE_SCHEMA_KEY, ACCEPTANCE_STORE_SCHEMA_VERSION],
                )
                .context("failed to stamp acceptance-store schema version")?;
        }
    }
    connection
        .execute_batch(ACCEPTANCE_SCHEMA_SQL)
        .context("failed to initialize acceptance-store schema")?;
    Ok(connection)
}

fn open_existing_acceptance_store(paths: &PublicationWorkspace) -> Result<Connection> {
    let store_path = paths.acceptance_store_path();
    if !store_path.is_file() {
        bail!(
            "acceptance store is missing: {}",
            normalize_path(&store_path)
        );
    }
    let connection = Connection::open_with_flags(
        &store_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("failed to open acceptance store {}", store_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set acceptance-store busy timeout")?;
    let schema = connection
        .query_row(
            "SELECT value FROM acceptance_store_meta WHERE key = ?1",
            params![STORE_SCHEMA_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to inspect acceptance-store schema version")?;
    if schema.as_deref() != Some(ACCEPTANCE_STORE_SCHEMA_VERSION) {
        bail!(
            "acceptance store {} has missing or incompatible schema marker {:?}; expected {ACCEPTANCE_STORE_SCHEMA_VERSION}",
            normalize_path(&store_path),
            schema
        );
    }
    Ok(connection)
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set acceptance-store busy timeout")?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )
        .context("failed to configure acceptance-store durability")?;
    Ok(())
}

fn validate_commit_inputs(
    decision: &AcceptanceStoreDecision,
    ledger_entries: &[ArticleAcceptanceLedgerEntry],
) -> Result<()> {
    if ledger_entries.is_empty() {
        bail!("publication-acceptance transaction requires at least one authorization");
    }
    if !is_sha256(&decision.decision_id) {
        bail!("publication-acceptance decision id must be a lowercase SHA-256 digest");
    }
    match decision.kind {
        AcceptanceDecisionKind::SingleArticle if decision.changeset_sha256.is_some() => {
            bail!("single-article acceptance cannot carry a changeset identity");
        }
        AcceptanceDecisionKind::ArticleChangeset => {
            if !decision.changeset_sha256.as_deref().is_some_and(is_sha256) {
                bail!("changeset acceptance requires a lowercase SHA-256 changeset identity");
            }
        }
        AcceptanceDecisionKind::SingleArticle => {}
    }
    if decision.human_editor_claim.trim().is_empty()
        || decision.editor_identity_assurance != EDITOR_IDENTITY_ASSURANCE
        || decision.decision != ACCEPTANCE_DECISION
        || decision
            .publication_authority
            .target_api_url
            .trim()
            .is_empty()
        || decision
            .publication_authority
            .site_adapter_id
            .trim()
            .is_empty()
        || !is_sha256(&decision.publication_authority.publication_policy_sha256)
    {
        bail!("publication-acceptance decision contains incomplete or invalid authority");
    }
    let _: serde_json::Value = serde_json::from_str(&decision.receipt_json)
        .context("publication-acceptance decision receipt must be valid JSON")?;

    let mut target_identities = BTreeSet::new();
    for entry in ledger_entries {
        validate_target_relative_path(&entry.target_relative_path)?;
        if entry.schema_version != ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION
            || entry.title.trim().is_empty()
            || !is_sha256(&entry.content_sha256)
            || entry.human_editor_claim != decision.human_editor_claim
            || entry.editor_identity_assurance != decision.editor_identity_assurance
            || entry.decision != decision.decision
            || entry.accepted_at_unix != decision.accepted_at_unix
            || entry.publication_authority.as_ref() != Some(&decision.publication_authority)
        {
            bail!("publication-acceptance authorization does not match its decision authority");
        }
        let _ = entry.resolved_warning_decision()?;
        match decision.kind {
            AcceptanceDecisionKind::SingleArticle if entry.changeset_decision.is_some() => {
                bail!("single-article authorization cannot carry a changeset binding");
            }
            AcceptanceDecisionKind::ArticleChangeset => {
                let binding = entry
                    .changeset_decision
                    .as_ref()
                    .context("changeset authorization is missing its decision binding")?;
                if binding.decision_id != decision.decision_id
                    || Some(binding.changeset_sha256.as_str())
                        != decision.changeset_sha256.as_deref()
                {
                    bail!("changeset authorization does not match its decision identity");
                }
            }
            AcceptanceDecisionKind::SingleArticle => {}
        }
        let target_identity = portable_target_identity(&entry.target_relative_path);
        if !target_identities.insert(target_identity) {
            bail!(
                "publication-acceptance transaction contains colliding target identities: {}",
                entry.target_relative_path
            );
        }
    }
    Ok(())
}

fn insert_decision(
    transaction: &Transaction<'_>,
    decision: &AcceptanceStoreDecision,
) -> Result<()> {
    let stored = StoredAcceptanceDecision {
        decision_id: decision.decision_id.clone(),
        kind: decision.kind,
        changeset_sha256: decision.changeset_sha256.clone(),
        accepted_at_unix: decision.accepted_at_unix,
        publication_authority: decision.publication_authority.clone(),
        receipt_sha256: compute_sha256(&decision.receipt_json),
        receipt_json: decision.receipt_json.clone(),
    };
    let inserted = transaction
        .execute(
            "INSERT OR IGNORE INTO acceptance_decisions (
                decision_id,
                decision_kind,
                changeset_sha256,
                accepted_at_unix,
                target_api_url,
                site_adapter_id,
                publication_policy_sha256,
                receipt_sha256,
                receipt_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                stored.decision_id,
                stored.kind.as_str(),
                stored.changeset_sha256,
                u64_to_i64(stored.accepted_at_unix, "acceptance timestamp")?,
                stored.publication_authority.target_api_url,
                stored.publication_authority.site_adapter_id,
                stored.publication_authority.publication_policy_sha256,
                stored.receipt_sha256,
                stored.receipt_json,
            ],
        )
        .context("failed to insert publication-acceptance decision")?;
    if inserted == 0 {
        let existing = load_decision_from_connection(transaction, &decision.decision_id)?;
        if existing != stored {
            bail!(
                "publication-acceptance decision id collision for {}",
                decision.decision_id
            );
        }
    }
    Ok(())
}

fn upsert_authorization(
    transaction: &Transaction<'_>,
    decision: &AcceptanceStoreDecision,
    ledger_entry: &ArticleAcceptanceLedgerEntry,
) -> Result<()> {
    let authority = ledger_entry
        .publication_authority
        .as_ref()
        .context("publication-acceptance authorization is missing authority")?;
    let ledger_json = serde_json::to_string(ledger_entry)
        .context("failed to encode publication-acceptance authorization")?;
    let target_identity = portable_target_identity(&ledger_entry.target_relative_path);
    transaction
        .execute(
            "INSERT INTO article_acceptance_authorizations (
                target_identity,
                decision_id,
                title,
                source_relative_path,
                target_relative_path,
                content_sha256,
                accepted_at_unix,
                target_api_url,
                site_adapter_id,
                publication_policy_sha256,
                ledger_sha256,
                ledger_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(target_identity) DO UPDATE SET
                decision_id = excluded.decision_id,
                title = excluded.title,
                source_relative_path = excluded.source_relative_path,
                target_relative_path = excluded.target_relative_path,
                content_sha256 = excluded.content_sha256,
                accepted_at_unix = excluded.accepted_at_unix,
                target_api_url = excluded.target_api_url,
                site_adapter_id = excluded.site_adapter_id,
                publication_policy_sha256 = excluded.publication_policy_sha256,
                ledger_sha256 = excluded.ledger_sha256,
                ledger_json = excluded.ledger_json",
            params![
                target_identity,
                decision.decision_id,
                ledger_entry.title,
                ledger_entry.source_relative_path,
                ledger_entry.target_relative_path,
                ledger_entry.content_sha256,
                u64_to_i64(ledger_entry.accepted_at_unix, "acceptance timestamp")?,
                authority.target_api_url,
                authority.site_adapter_id,
                authority.publication_policy_sha256,
                compute_sha256(&ledger_json),
                ledger_json,
            ],
        )
        .context("failed to write publication-acceptance authorization")?;
    Ok(())
}

fn decode_authorization_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAuthorizationRow> {
    let accepted_at_unix = row.get::<_, i64>(5)?;
    if accepted_at_unix < 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(
            5,
            accepted_at_unix,
        ));
    }
    Ok(StoredAuthorizationRow {
        decision_id: row.get(0)?,
        title: row.get(1)?,
        source_relative_path: row.get(2)?,
        target_relative_path: row.get(3)?,
        content_sha256: row.get(4)?,
        accepted_at_unix: accepted_at_unix as u64,
        publication_authority: ArticlePublicationAuthority {
            target_api_url: row.get(6)?,
            site_adapter_id: row.get(7)?,
            publication_policy_sha256: row.get(8)?,
        },
        ledger_sha256: row.get(9)?,
        ledger_json: row.get(10)?,
    })
}

fn validate_authorization_row(
    row: &StoredAuthorizationRow,
) -> Result<ArticleAcceptanceLedgerEntry> {
    if !is_sha256(&row.ledger_sha256) || compute_sha256(&row.ledger_json) != row.ledger_sha256 {
        bail!("publication-acceptance authorization has a corrupt ledger fingerprint");
    }
    let ledger_entry: ArticleAcceptanceLedgerEntry = serde_json::from_str(&row.ledger_json)
        .context("failed to decode transactional publication acceptance")?;
    if ledger_entry.schema_version != ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION
        || ledger_entry.title != row.title
        || ledger_entry.source_relative_path != row.source_relative_path
        || ledger_entry.target_relative_path != row.target_relative_path
        || ledger_entry.content_sha256 != row.content_sha256
        || ledger_entry.accepted_at_unix != row.accepted_at_unix
        || ledger_entry.publication_authority.as_ref() != Some(&row.publication_authority)
    {
        bail!("publication-acceptance store row does not match its bound ledger payload");
    }
    Ok(ledger_entry)
}

fn load_decision_from_connection(
    connection: &Connection,
    decision_id: &str,
) -> Result<StoredAcceptanceDecision> {
    if !is_sha256(decision_id) {
        bail!("publication-acceptance decision id must be a lowercase SHA-256 digest");
    }
    let stored = connection
        .query_row(
            "SELECT
                decision_id,
                decision_kind,
                changeset_sha256,
                accepted_at_unix,
                target_api_url,
                site_adapter_id,
                publication_policy_sha256,
                receipt_sha256,
                receipt_json
             FROM acceptance_decisions
             WHERE decision_id = ?1",
            params![decision_id],
            |row| {
                let accepted_at_unix = row.get::<_, i64>(3)?;
                if accepted_at_unix < 0 {
                    return Err(rusqlite::Error::IntegralValueOutOfRange(
                        3,
                        accepted_at_unix,
                    ));
                }
                let kind = row.get::<_, String>(1)?;
                Ok((
                    row.get::<_, String>(0)?,
                    kind,
                    row.get::<_, Option<String>>(2)?,
                    accepted_at_unix as u64,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()
        .context("failed to read publication-acceptance decision")?
        .with_context(|| format!("publication-acceptance decision is missing: {decision_id}"))?;
    let decision = StoredAcceptanceDecision {
        decision_id: stored.0,
        kind: AcceptanceDecisionKind::parse(&stored.1)?,
        changeset_sha256: stored.2,
        accepted_at_unix: stored.3,
        publication_authority: ArticlePublicationAuthority {
            target_api_url: stored.4,
            site_adapter_id: stored.5,
            publication_policy_sha256: stored.6,
        },
        receipt_sha256: stored.7,
        receipt_json: stored.8,
    };
    if decision.decision_id != decision_id
        || !is_sha256(&decision.receipt_sha256)
        || compute_sha256(&decision.receipt_json) != decision.receipt_sha256
    {
        bail!("publication-acceptance decision has a corrupt receipt binding");
    }
    Ok(decision)
}

fn portable_target_identity(target_relative_path: &str) -> String {
    normalize_relative_path(target_relative_path).to_lowercase()
}

fn legacy_json_ledger_path(
    paths: &PublicationWorkspace,
    target_relative_path: &str,
) -> Result<PathBuf> {
    validate_target_relative_path(target_relative_path)?;
    Ok(paths.state_dir.join("acceptance-ledger").join(format!(
        "{}.json",
        normalize_relative_path(target_relative_path)
    )))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn u64_to_i64(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{label} does not fit into SQLite INTEGER"))
}
