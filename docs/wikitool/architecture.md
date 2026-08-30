# Wikitool architecture

Wikitool is a general-purpose MediaWiki substrate for humans and external agents. It does not
embed a model, a house editorial voice, or target-wiki doctrine. The architecture separates
mechanical truth from editorial judgment so both sides can evolve without pretending one can
replace the other.

## Design principles

1. **MediaWiki is the live content authority.** Local wikitext is a synchronized working copy.
   Catalog indexes, capability snapshots, and generated briefs are rebuildable projections;
   revision baselines and publication decisions are separate durable authority stores.
2. **Evidence identities travel with retrieval.** Paths, revision IDs, hashes, limits, and
   incomplete-scope signals matter more than fluent summaries or retrieval rank.
3. **The core owns decidable mechanics.** Transport, parsing, revision comparison, template
   contracts, deterministic lint, atomic local writes, and exact-content interlocks belong in Rust.
4. **Skills own judgment.** Research strategy, claim-source mapping, source suitability, prose,
   due weight, BLP care, and reader value remain inspectable agent procedures outside the binary.
5. **Sites opt into policy explicitly.** A target-neutral embedded adapter works without a
   project adapter. Project policy is loaded only through `[adapter].path`; there is no executable-ancestor
   or neighboring-repository discovery.
6. **Human authority is not identity theater.** Promotion records an exact-content decision and a
   caller-supplied named-human claim, but a free-form local label is not authentication or
   cryptographic proof.
7. **Unsafe uncertainty fails locally.** Unknown configuration fields, missing adapter resources,
   conflicting revisions, changed accepted bytes, and ambiguous writes surface as errors.

## Layers and ownership

```text
human editor
  publication judgment; exact final-prose acceptance
        |
agent skills
  interview; research-to-claim; writing; independent prose review
        |
project site adapter
  typed site policy + hashed supplemental guidance
        |
thin wikitool CLI + application core
  workspace; revision CAS; indexing; context packets; lint; authority stores
        |
mediawiki_protocol + bounded source acquisition
  authenticated API transport; read/write constraints; source capture
        |
MediaWiki + local source files + external source documents
```

### Protocol, sync, publication, application core, and CLI

`crates/mediawiki_protocol` owns MediaWiki API models, authentication, reads, writes, rendered-page
decoding, and the no-write-retry transport contract. It has no workspace, adapter, acceptance,
catalog, or agent concepts. `crates/wikitool_sync` owns the target-bound durable sync repository,
pull/plan/diff/push/delete state transitions, exact preview-plan identities, and mutation
reconciliation over narrow project paths, target identity, protocol, and publication-preflight
ports. `crates/wikitool_publication` implements the encyclopedic preflight and owns transactional
acceptance/changeset authority; sync does not know what an article or human acceptance is.
`crates/wikitool_core` adapts project configuration, catalog effects, site adapters, lint, source
capture, and authoring-support packets onto those lower layers. `crates/wikitool` owns command
composition and bounded presentation. The core may report that a citation URL
matches a configured review rule; it may not declare that source universally reliable or infer a
prose verdict from the match.

`crates/mediawiki_html_to_wikitext` is a separate library boundary for deterministic HTML5 DOM
traversal into conservative MediaWiki primitives. It accepts caller-supplied source interpretation,
target template policy, and a separately verified media inventory. The source profile declares
observed structural semantics such as infobox table, title-row, and field-layout forms; the target
profile declares the destination template vocabulary. The default CLI exposes only this normalized
profiled compiler. It does not decode an acquisition bundle, choose a site policy, emit a
preservation receipt, or write to MediaWiki. Producer adapters and site-specific projection
companions remain outside the core.

`crates/mediawiki_wikitext` is the corresponding boundary for exact revision-bound MediaWiki
source. It verifies a producer-neutral page receipt, applies source-reported namespace and redirect
syntax, and returns bounded span-addressed templates, parameters, links, redirects, and protected
literal regions. Rewrites are caller-selected node replacements and overlapping authority is an
error. The crate does not enumerate a corpus, decode a private producer manifest, expand source
templates, choose destination semantics, or emit a preservation transform receipt.

Mechanical checks include wikitext structure, citation placement, required templates, configured
placeholder fragments, extension availability, link/index integrity, and revision-bound sync.
They deliberately exclude reader interest, prose specificity, due weight, source entailment,
relationship importance, and sensitive-claim adequacy.

### Site adapter

`.wikitool/config.toml` may name one project-owned adapter:

```toml
[adapter]
path = "site-adapter/site-adapter.toml"
```

The `site_adapter_v2` TOML is strict and may configure:

- short-description, quality-banner, appendix, and wikitext mechanics;
- citation template preferences and URL source-review matchers;
- infobox preferences and deterministic lint values;
- parser-tag, parser-function, template, and module contracts;
- relative supplemental guidance documents.

The adapter and each named guidance document receive a content hash in the resolved adapter
identity. Markdown is context for agents, not executable policy. Source-review rules are routing
signals, not bans.
The embedded `mediawiki-generic` adapter has no target URL, branded prose, or site-specific source
list.

### Agent skills

Canonical procedures live under `ai-pack/codex_skills/`:

- `wikitool-operator` operates the mechanical surface and preserves mutation boundaries.
- `wiki-interview` asks from supplied material and the conversation rather than a canned
  questionnaire, then stores a neutral ledger.
- `wiki-writing` requires inspected source documents and a claim-source map before prose.
- `prose-review` reconstructs claims independently, opens the exact sources, tests weight and BLP
  concerns, and answers whether a reader would want the article.

Claude files are harness adapters to those same packages. Packaging never replaces the public
skills with host-project skills. An explicit host can contribute only a site-adapter supplement.

## Authoring flow

```text
local wiki context + inspected external sources + human notes
                           |
                           v
          article_scout_v1 machine context surface
                           |
                           v
          claim-source map and explicit unknowns
                           |
                           v
       wiki-writing skill drafts body, then the lead
                           |
                           v
      prose-review skill + deterministic article lint
                           |
                           v
        named human reads the exact final prose
                           |
                           v
 article_acceptance_ledger_v3 -> promotion -> revision-bound push
```

`article scout --format json --view brief` returns local state, context coverage,
stable context identities, comparable-page observations, observed categories, available
templates, and blocking ledger gaps. It does not contain prompt prose, a recommended editorial
action, a canned question agenda, or a quality claim. `retrieval_ready` means the configured local
artifacts are current enough to retrieve; it does not mean the topic is researched.

Local and neighboring articles can establish target-wiki state and internal relationships. They
are not automatically independent evidence for external factual claims. Model memory, search
snippets, retrieval rank, and fluent synthesis are never evidence.

## Interview ledger

`interview init|validate|show|audit|open-item` owns deterministic paths, a parseable
frontmatter schema, neutral sections, structured negative evidence, and freshness. The CLI creates
no question agenda and does not grade the interview's editorial sufficiency.

Human notes, source leads, exclusions, and firsthand knowledge remain distinct. Validation proves
only that the ledger is structurally usable. A source lead must still be inspected; a human claim
must retain its provenance and uncertainty; neither becomes publication acceptance.

## Prose review boundary

The core no longer carries phrase heuristics for “AI prose” or substring rules for host-wiki
relationships. Those checks had poor precision and trained operators to ignore warnings. The
review skill instead locates concrete failures:

- a factual or implied claim not entailed by its source;
- citation laundering or compressed tertiary sourcing;
- a sensitive claim without direct high-quality support;
- a true but gratuitous or disproportionate relationship;
- vague synthesis that spends attention without teaching a sourced distinction;
- structure or pacing unsupported by the evidence volume.

Mechanical lint runs after this review and remains a separate result. Zero lint findings cannot
certify truth, originality, readability, or source fidelity.

## Publication-acceptance store

Changed non-redirect Main-namespace prose requires a matching authorization row in
`.wikitool/acceptance/acceptance.sqlite3` before promotion and push. Its
`article_acceptance_ledger_v3` payload binds:

- source and target paths, title, and exact content SHA-256;
- the self-reported human editor claim and `self_reported_unverified` assurance;
- the acceptance timestamp;
- prose origin and the `accepted_for_main_namespace_promotion` decision;
- lint counts and whether warnings were explicitly accepted;
- the normalized MediaWiki API endpoint and full SHA-256 identity of the active site-adapter
  policy plus every declared guidance document.

Any byte, configured target, or publication-policy change invalidates the row; `--force` cannot
bypass these bindings. Legacy JSON ledgers are detected and rejected clearly as historical,
unbound evidence; they are never a parallel authority path. The store is useful accountability and
a workflow interlock. It is not identity authentication, proof that the named person actually
reviewed the article, or a machine quality certificate.

For a coherent multi-article review, `article changeset prepare` freezes canonical title, source
and target paths, exact content hash, prose origin, and complete lint evidence for every item.
`article changeset accept` records one named decision and every independently invalidating member
authorization in a single SQLite transaction. A process failure or late write error rolls back the
whole replacement and preserves all prior authorizations. The decision timestamp is part of the
decision digest and must match every member row. Push reports expose the verified content hash,
acceptance timestamp,
prose origin, identity-assurance level, warning decision, target/policy authority, and changeset
identity, but claimed editor names are never added to public MediaWiki edit summaries.

## Sync and local durability

Existing-page edits send MediaWiki `baserevid`; creations send `createonly`. Remote presence is
reported explicitly. A locally modified page deleted remotely is a conflict; an operator can
recreate it only with `--force`, and the request remains `createonly`. Deletions re-fetch revision
identity immediately before mutation and refuse a mismatch observed by that check even under
`--force`. MediaWiki offers no revision-conditional delete parameter, so the check narrows but
cannot close the time-of-check/time-of-use window before `action=delete`. An already absent remote
page causes local sync-state reconciliation without a delete request. Mutating requests are not
generically retried, preventing an ambiguous write from being silently replayed.
The MediaWiki `bot` marker is an explicit `wiki.mark_edits_as_bot` transport setting, independent
of who wrote or reviewed the prose.

Push and standalone delete preview by default and bind apply to a SHA-256 plan identity over the
normalized target and the exact selected local, sync, remote, policy, and summary inputs relevant
to the operation. Before any write request, sync persists a target-bound mutation intent; request
start, response identity, current remote observation, reconciliation, and local state advancement
are distinct durable phases. Ledger and base-snapshot changes commit together. Ambiguous outcomes
block later writes for that title and expose public list/show/reconcile commands; no recovery path
replays the mutation. When neither content/revision lineage nor an exact delete log can prove the
outcome, an operator may close the receipt as `operator_closed_unresolved`. That closure has its
own actor/reason/timestamp provenance, makes no assertion about remote success, invalidates the
title's baseline, and requires a fresh `pull --full --all` before another write. Delete-local
staging and exact backup validation are part of the same durable recovery protocol; disposable
catalog cleanup cannot hide a terminal mutation receipt.

Promotions, imports, interview state, source-access sessions, configuration patches, pull writes, and
deletion backups use same-directory atomic replacement. Publication decisions and member
authorizations use the separate durable acceptance store and commit transactionally. The catalog
database remains disposable and can be rebuilt from source files and live state. Sync revision
identity, incremental checkpoints, and exact base snapshots live in a separate durable SQLite
store; catalog reset and pull/catalog refresh operations preserve both durable stores, and sync
planning fails closed when sync identity is absent. Only an explicitly global, successful
`pull --full --all` writes global baseline
authority; scoped or category pulls and legacy migrations may preserve rows but cannot prove
coverage and therefore cannot unlock global diff/status/review/push. Establishment atomically binds
the store to a normalized MediaWiki API endpoint. Every planning and publication read verifies that
identity, and target mismatch fails before network conflict hydration or mutation; `--force` cannot
waive it.

## Source-acquisition boundary

`source` uses one outbound HTTP policy: HTTP(S) only, no embedded credentials, no local or
special-use targets, and validation across redirects. Session cookies honor secure, host-only, and
path constraints. On Windows, their directory and files use a native, protected, current-user-only
DACL. On Unix, they use verified current-effective-user ownership with directory mode `0700` and
file mode `0600`. The atomic staging file is protected before secret bytes are written, and both
platforms verify protection again after publication and before loading; uncertainty fails closed.
Cache keys include extractor and session identity and have bounded lifetimes.

Source acquisition does not establish source suitability. The writing and review skills must open
the work, bind claims to exact passages, preserve source role, and disclose inaccessible material.

## Evaluation boundary

`crates/wikitest` is a source-resident evaluator, not a library back door or a prose grader inside
Wikitool. It invokes the public executable in isolated projects or an explicitly admitted
read-only host root. Its generic catalog covers mechanical article state transitions, a local
MediaWiki HTTP sync boundary, and synthetic local-index retrieval; a host repository may add supplemental real-corpus scenarios without
embedding site policy in either general-purpose crate.

Prose assignments freeze the inspected sources and canonical skill bytes, accept an external
author's exact article and claim-source map, then expose a blinded packet to a differently
identified reviewer. Controlled cases test stable verdict axes and disposition, not secret phrase
matching. Receipts bind both the exact Wikitest driver and the evaluated Wikitool binary. Every
run-producing command re-inspects that receipt before success, and state advances preflight the
recorded identities so a stale evaluator cannot partially mutate a prose run. CI can
deterministically run mechanics, catalog fixtures, and packet construction; model-backed prose
performance remains an explicit dogfood campaign whose participant and receipt evidence must be
reported separately. Prose packet preparation records prepared coverage only; demonstrated
coverage exists only after `prose evaluate-suite` replays completed independent review receipts
and passes any held-out oracle.

## Release boundary

Release archives contain a target-neutral AI pack, canonical skills, and a versioned built-in
adapter catalog with generic and Remilia Wiki templates. Catalog presence is inert: only a
project-relative `[adapter].path` selects policy, and no bundled adapter supplies an endpoint.
Receipt inspection replays deterministic observations and capability-to-step bindings. It reports
the self-contained artifact set as unanchored rather than claiming authenticity; release evidence
needs an independently published immutable digest or signature.

`--host-project-root` is explicit and accepts only the typed `wikitool_adapter/site-adapter.toml` plus
the guidance files it declares as a supplement under `site_adapters/project/`; undeclared neighbor
files are not shipped. The supplement never replaces public `CLAUDE.md`, `AGENTS.md`, rules,
wrappers, or skills. Unknown, malformed, traversing, or incomplete host adapter state fails
packaging.

Wikitest is intentionally not packaged in end-user release archives. Its authority depends on the
source catalog, controlled inputs, and public skill tree that accompany a source checkout; the
shipped runtime remains the Wikitool binary and agent pack being evaluated.

Contextmink remains a separately versioned, hash-pinned upstream release pack. Wikitool contains no
Contextmink source fork and no duplicate installer.

## Known limitations

- The acceptance editor label is unauthenticated. Strong identity would require an external signed
  review or authenticated interactive system, not another CLI string.
- Sync currently implements MediaWiki `first-letter` title identity and a fixed built-in namespace
  prefix set. Case-sensitive namespaces are not a supported write target until siteinfo namespace
  case policy participates in planning, ledger, mutation, and target identities.
- Target-specific preservation-projection artifacts developed alongside the host archive campaign
  are intentionally excluded from the core crate, generated reference, and release package. The
  default binary exposes generic profiled DOM compilation, but private producer input and
  target-specific receipt schemas belong to separately versioned companions.
- Wikitext parsing is deterministic and bounded, not a complete replacement for MediaWiki's
  production parser; rendered behavior must be checked through `wiki render-check` where relevant.
- Wikitest provides replayable structural and closed-world evidence, but model performance must be
  sampled across agents and real source packets; one successful campaign or prompt conformance is
  not a universal quality claim.

## Development contract

When behavior changes:

1. update the typed model and schema version at the boundary that changed;
2. test behavior rather than only command construction or prose snapshots;
3. regenerate `docs/wikitool/reference.md` from clap help;
4. run `docs audit` to verify target neutrality, skill package shape, adapter routing, and generated
   reference freshness;
5. run the relevant Wikitest suites, replay their receipts, and externally anchor release evidence;
6. package both the generic AI pack and an explicit host-adapter supplement;
7. keep live writes out of tests unless the test is explicitly authorized and revision-bound.
