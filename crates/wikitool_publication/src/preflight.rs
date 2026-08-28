use std::fs;

use anyhow::{Context, Result};
use wikitool_sync::{
    PreparedPublication, PublicationAuthorityBinding, PublicationCandidate, PublicationPreflight,
    PublicationProvenance,
};

use crate::PublicationWorkspace;
use crate::acceptance::{ArticlePublicationAuthority, load_accepted_article};

/// Publication policy for encyclopedic MediaWiki projects: ordinary namespace
/// content is read as-is, while non-redirect Main articles must resolve to an
/// exact-content authorization from the durable acceptance store.
#[derive(Debug, Clone)]
pub struct EncyclopedicPublicationPreflight {
    workspace: PublicationWorkspace,
    authority: ArticlePublicationAuthority,
}

impl EncyclopedicPublicationPreflight {
    pub fn new(workspace: PublicationWorkspace, authority: ArticlePublicationAuthority) -> Self {
        Self {
            workspace,
            authority,
        }
    }
}

impl PublicationPreflight for EncyclopedicPublicationPreflight {
    fn prepare(&self, candidate: PublicationCandidate<'_>) -> Result<PreparedPublication> {
        if candidate.namespace == "Main" && !candidate.is_redirect {
            let accepted = load_accepted_article(
                &self.workspace,
                &self.authority,
                candidate.absolute_path,
                candidate.title,
                candidate.relative_path,
            )?;
            let provenance = accepted.provenance()?;
            return Ok(PreparedPublication {
                content: accepted.content,
                provenance: Some(PublicationProvenance {
                    content_sha256: provenance.content_sha256,
                    accepted_at_unix: provenance.accepted_at_unix,
                    prose_origin: provenance.prose_origin.as_str().to_string(),
                    editor_identity_assurance: provenance.editor_identity_assurance,
                    warning_decision: provenance.warning_decision.as_str().to_string(),
                    decision_id: provenance.decision_id,
                    changeset_sha256: provenance.changeset_sha256,
                    publication_authority: provenance.publication_authority.map(|authority| {
                        PublicationAuthorityBinding {
                            target_api_url: authority.target_api_url,
                            site_adapter_id: authority.site_adapter_id,
                            publication_policy_sha256: authority.publication_policy_sha256,
                        }
                    }),
                }),
            });
        }

        let content = fs::read_to_string(candidate.absolute_path).with_context(|| {
            format!(
                "failed to read publication candidate {}",
                candidate.absolute_path.display()
            )
        })?;
        Ok(PreparedPublication {
            content,
            provenance: None,
        })
    }
}
