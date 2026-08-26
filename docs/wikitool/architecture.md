# Wikitool architecture

Wikitool is an evidence, authoring, review, and guarded synchronization system for MediaWiki. It
does not embed an LLM backend. External agents may use its typed outputs and shipped skills to write
real encyclopedic prose; wikitool owns the evidence and publication boundaries around that work.

## System boundaries

- `crates/wikitool` owns CLI parsing, command dispatch, compact presentation, and release surfaces.
- `crates/wikitool_core` owns MediaWiki IO, revision-aware sync, indexing, retrieval, typed profile
  policy, deterministic parsing, linting, acceptance verification, and reusable models.
- `ai-pack/` owns the coauthoring contract and editorial guidance. It explains how an external agent
  turns evidence into prose; it does not duplicate CLI reference text.
- `docs/wikitool/reference.md` is generated from clap help and is the flag authority.
- `.wikitool/data/wikitool.db` is a rebuildable projection. Source files, live MediaWiki revisions,
  typed profile policy, interview/open-item ledgers, source artifacts, and acceptance receipts are
  the authorities at their respective boundaries.

Large CLI families use thin facades and focused submodules. Shared behavior belongs in core only
when multiple lanes have the same constraints. Machine policy must not be inferred by searching
Markdown examples.

## Coauthoring pipeline

The intended flow is:

```text
live/local wiki + external sources + human knowledge
                     |
                     v
       bounded evidence and explicit unknowns
                     |
                     v
      article_start_v3 coauthoring contract
                     |
                     v
     external agent or human drafts real prose
                     |
                     v
   reader/source review + deterministic lint/fix
                     |
                     v
      article_acceptance_v2 human decision
                     |
                     v
       revision-bound promotion and push
```

Wikitool supplies knowledge to the writer, but retrieval order and profile defaults do not own
editorial judgment. `knowledge article-start --format json --view brief` exposes a typed contract:
agent prose drafting is allowed;
model output is not evidence; structure derives from the evidenced factual spine; adjacent-entity
relationships must be important, supported, and proportionate; exact human acceptance is required
for publication.

The compact result includes evidence coverage, stable source paths and content hashes, local fit
signals, open questions, constraints, and follow-up commands. It excludes the exact subject from
comparable-page structure and filters configured relationship headings. Comparable outlines remain
observations rather than templates.

## Publication acceptance

Promotion and changed non-redirect Main-namespace pushes require an `article_acceptance_v2`
receipt. The receipt binds:

- title, source path, and target path;
- exact article SHA-256;
- named human editor;
- truthful prose origin, including `agent_draft` and `collaborative_draft`;
- zero-error lint summary and the human warning decision;
- an attestation that the exact prose was read and accepted;
- an editorial attestation that it was judged specific, readable, proportionate, and source-bound.

Any content change makes the receipt stale. `--force` cannot bypass the gate. The identity is an
explicit audit assertion, not cryptographic authentication, so shipped guidance also forbids an
agent from self-attesting. The command is `wikitool article accept`.

This gate does not make prose quality mechanically decidable. It makes the human publication
decision visible and binds it to the exact bytes.

## Prose diagnostics

Lint remains deterministic and diagnostic. `style.synthetic_phrase` is a suggestion to reread the
underlying thought; `editorial.forced_relationship_frame` is a warning that requires editorial
judgment. Neither finding is evidence of AI authorship, and zero findings do not establish truth,
source fidelity, proportion, or reader value.

The shipped style review therefore uses hard source/subject failures and adversarial reader
questions, not a vocabulary detector. It asks whether paragraphs teach concrete subject-specific
facts, citations support nearby claims, coverage reflects evidence rather than retrieval
availability, and the lead defines the subject without gratuitous host-wiki framing.

## Typed profile policy

Machine-consumed policy lives in `ai-pack/writing_context/profile.toml`. Wikitool validates the
profile schema and consumes typed authoring, citation, category, lint, golden-set, and extension
contracts. Markdown contributes human guidance and provenance hashes but cannot silently create
global defaults.

The Remilia profile has no global preferred category. Relationship-to-Remilia headings are
editorial warnings, not required structure. `parent_group = Remilia` is a template value for actual
Remilia projects, not an inferred relationship for adjacent people or works.

## Evidence and readiness

Agent-facing output is compact, bounded, and explicit about incomplete scope. Brief views expose
selection, source paths, hashes, readiness, degradation, and targeted follow-up commands. Expanded
bodies are opt-in through `--view full`. Retrieval lanes use limits, token budgets, page caps, and
content-derived evidence identities rather than ordinal IDs.

`knowledge status` reports `drafting_ready` only when both content and documentation artifacts use
the current generation and their recorded imports are clean. `content_ready` means the current
content projection exists without a current clean docs profile. Neither state says that a topic has
adequate sources or that prose is ready to publish.

Local articles and comparable pages are claim and fit surfaces, not automatic independent
authority. External source selection remains an editorial/research step. Source extraction should
retain enough locator and provenance information to audit every load-bearing claim.

## Interview ledger

`knowledge interview init|validate|show|audit|open-item` owns deterministic paths, frontmatter
validation, structured open items, freshness, and compact handoff summaries. The conversation is
owned by the agent and human together.

A brief may define the article object, record target-wiki testimony, classify source leads,
preserve exclusions, and propose a draft plan. It is not automatically independent evidence or
publication acceptance. An external agent may draft from a validated brief after inspecting the
relevant sources and preserving open negative/do-not-assert items.

## Sync safety

Push planning hydrates the observed remote revision identity. Existing-page edits send
`baserevid`; creates send `createonly`. Generic mutation retries are disabled so an ambiguous write
is not silently replayed. A forced push may waive a preflight conflict only with explicit approval;
the write remains compare-and-swap bound to the revision observed for that attempt.

The remaining durability gap is post-write reconciliation: the API response is not yet recorded as
a durable remote mutation receipt, and a later fetch can observe a subsequent editor. Persist the
edit response revision ID and reconcile that exact revision before treating this edge as closed.

## Outbound research safety

Research requests use a shared outbound policy: HTTP(S) only, no embedded credentials, no local or
special-use targets, and validation at every redirect. Cookies honor Secure, host-only, and path
boundaries. Cache keys include schema, extractor, user-agent, and session fingerprints; entries
have a bounded lifetime and replacement writes avoid partial files.

Two limitations remain explicit. DNS validation and the HTTP connection do not yet share a pinned
resolution, leaving a DNS-rebinding window. On Windows, session-cookie files are masked in CLI
output but do not yet receive an explicit user-only ACL.

## Contextmink boundary

Contextmink is an independent project-generic transcript guard. Wikitool release bundles consume a
version-pinned upstream Contextmink release pack staged by `scripts/fetch_contextmink.sh` and
verified against repository-owned archive hashes. Wikitool carries no Contextmink source fork and
no second installer; the bundled Contextmink binary owns receipt-backed project setup through
`setup-project`.

## Development contract

When behavior changes:

1. update clap help and regenerate `docs/wikitool/reference.md`;
2. update the operator guide and thin Claude/Codex wrappers;
3. keep `ai-pack/AGENTS.md` and `ai-pack/CLAUDE.md` byte-identical;
4. update typed policy and writing context when editorial behavior changes;
5. add tests for the authority boundary and outcome, not incidental wording.

Use deterministic state-machine or character-by-character parsing for wikitext and HTML. Do not
introduce regex parsing at those boundaries.
