# Wikitool architecture

Wikitool is a general-purpose MediaWiki substrate for humans and external agents. It does not
embed a model, a house editorial voice, or target-wiki doctrine. The architecture separates
mechanical truth from editorial judgment so both sides can evolve without pretending one can
replace the other.

## Design principles

1. **MediaWiki is the live content authority.** Local wikitext is a synchronized working copy;
   SQLite, search indexes, capabilities, and generated briefs are rebuildable projections.
2. **Evidence identities travel with retrieval.** Paths, revision IDs, hashes, limits, and
   incomplete-scope signals matter more than fluent summaries or retrieval rank.
3. **The core owns decidable mechanics.** Transport, parsing, revision comparison, template
   contracts, deterministic lint, atomic local writes, and exact-content interlocks belong in Rust.
4. **Skills own judgment.** Research strategy, claim-source mapping, source suitability, prose,
   due weight, BLP care, and reader value remain inspectable agent procedures outside the binary.
5. **Sites opt into policy explicitly.** A target-neutral embedded profile works without an
   adapter. Project policy is loaded only through `[adapter].path`; there is no executable-ancestor
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
wikitool core and CLI
  MediaWiki IO; revision CAS; parsing; indexing; evidence; lint; ledgers
        |
MediaWiki + local source files + external source documents
```

### Core

`crates/wikitool_core` owns reusable behavior and typed models. `crates/wikitool` owns command
parsing, bounded presentation, and maintainer packaging. The core may report that a citation URL
matches a configured review rule; it may not declare that source universally reliable or infer a
prose verdict from the match.

Mechanical checks include wikitext structure, citation placement, required templates, configured
placeholder fragments, extension availability, link/index integrity, and revision-bound sync.
They deliberately exclude reader interest, prose specificity, due weight, source entailment,
relationship importance, and sensitive-claim adequacy.

### Site adapter

`.wikitool/config.toml` may name one project-owned adapter:

```toml
[adapter]
path = "site-adapter/profile.toml"
```

The `site_adapter_v1` TOML is strict and may configure:

- short-description, quality-banner, appendix, and wikitext mechanics;
- citation template preferences and URL source-review matchers;
- infobox preferences and deterministic lint values;
- parser-tag, parser-function, template, and module contracts;
- relative supplemental guidance documents.

The adapter and each named guidance document receive a content hash in the site profile. Markdown
is context for agents, not executable policy. Source-review rules are routing signals, not bans.
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
local wiki evidence + inspected external sources + human notes
                           |
                           v
          article_start_v4 machine evidence surface
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
 article_acceptance_ledger_v1 -> promotion -> revision-bound push
```

`knowledge article-start --format json --view brief` returns local state, evidence coverage,
stable evidence identities, comparable-page observations, observed categories, available
templates, and blocking ledger gaps. It does not contain prompt prose, a recommended editorial
action, a canned question agenda, or a quality claim. `drafting_ready` means the configured local
artifacts are current enough to retrieve; it does not mean the topic is researched.

Local and neighboring articles can establish target-wiki state and internal relationships. They
are not automatically independent evidence for external factual claims. Model memory, search
snippets, retrieval rank, and fluent synthesis are never evidence.

## Interview ledger

`knowledge interview init|validate|show|audit|open-item` owns deterministic paths, a parseable
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

## Acceptance ledger

Changed non-redirect Main-namespace prose requires a matching local
`article_acceptance_ledger_v1` entry before promotion and push. The entry binds:

- source and target paths, title, and exact content SHA-256;
- the self-reported human editor claim and `self_reported_unverified` assurance;
- prose origin and the `accepted_for_main_namespace_promotion` decision;
- lint counts and whether warnings were explicitly accepted.

Any byte change invalidates the entry; `--force` cannot bypass the content binding. The ledger is
useful accountability and a workflow interlock. It is not identity authentication, proof that the
named person actually reviewed the article, or a machine quality certificate.

## Sync and local durability

Existing-page edits send MediaWiki `baserevid`; creations send `createonly`. Remote presence is
reported explicitly. A locally modified page deleted remotely is a conflict; an operator can
recreate it only with `--force`, and the request remains `createonly`. Deletions re-fetch revision
identity immediately before mutation and refuse a same-run race even under `--force`. An already
absent remote page causes local sync-state reconciliation without a delete request. Mutating
requests are not generically retried, preventing an ambiguous write from being silently replayed.
The MediaWiki `bot` marker is an explicit `wiki.mark_edits_as_bot` transport setting, independent
of who wrote or reviewed the prose.

Acceptance entries, promotions, imports, interview state, research sessions, configuration
patches, pull writes, and deletion backups use same-directory atomic replacement. The database is
still disposable and can be rebuilt from source files and live state.

## Research boundary

Research uses one outbound HTTP policy: HTTP(S) only, no embedded credentials, no local or
special-use targets, and validation across redirects. Session cookies honor secure, host-only, and
path constraints. Cache keys include extractor and session identity and have bounded lifetimes.

Source acquisition does not establish source suitability. The writing and review skills must open
the work, bind claims to exact passages, preserve source role, and disclose inaccessible material.

## Evaluation boundary

`crates/wikitest` is a source-resident evaluator, not a library back door or a prose grader inside
Wikitool. It invokes the public executable in isolated projects or an explicitly admitted
read-only host root. Its generic catalog covers mechanical article state transitions and synthetic
local-index retrieval; a host repository may add supplemental real-corpus scenarios without
embedding site policy in either general-purpose crate.

Prose assignments freeze the inspected sources and canonical skill bytes, accept an external
author's exact article and claim-source map, then expose a blinded packet to a differently
identified reviewer. Controlled cases test stable verdict axes and disposition, not secret phrase
matching. Receipts bind both the exact Wikitest driver and the evaluated Wikitool binary. Every
run-producing command re-inspects that receipt before success, and state advances preflight the
recorded identities so a stale evaluator cannot partially mutate a prose run. CI can
deterministically run mechanics, knowledge fixtures, and packet construction; model-backed prose
performance remains an explicit dogfood campaign whose participant and receipt evidence must be
reported separately.

## Release boundary

Release archives contain a target-neutral AI pack, generic adapter example, and canonical skills.
`--host-project-root` is explicit and accepts only the typed `wikitool_adapter/profile.toml` plus
the guidance files it declares as a supplement under `site_adapter/project/`; undeclared neighbor
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
- Post-write reconciliation does not yet persist the edit-response revision as a durable mutation
  receipt before later observation.
- DNS validation and the HTTP connection do not yet share a pinned resolution, leaving a bounded
  DNS-rebinding window.
- Windows research-session files mask values in output but do not yet receive an explicit
  user-only ACL.
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
5. run the relevant Wikitest suites and retain their self-inspected receipts;
6. package both the generic AI pack and an explicit host-adapter supplement;
7. keep live writes out of tests unless the test is explicitly authorized and revision-bound.
