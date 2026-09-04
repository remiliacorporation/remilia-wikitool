# Wikitest capability matrix

Wikitest is an opt-in stress and evaluation laboratory. The matrix names what each campaign can
demonstrate and, equally importantly, what it cannot. Normal release correctness remains owned by
Cargo tests, lint, generated-reference checks, CLI integration checks, and release-dispatch checks.

## Deterministic capability campaign

Run `wikitest suite wikitool-capabilities --require-all` locally or dispatch the manual
`Wikitest capability campaign` workflow. The suite contains 11 isolated scenarios and requires all
78 declared capability slices.

| Scenario | Capability family | Important stress patterns |
|---|---|---|
| `mechanical-article-lifecycle` | Article state | Initialization, adapter binding, lint/fix, exact acceptance, promotion, stale acceptance rejection |
| `catalog-indexed-retrieval` | Local retrieval | Wikitext parsing, internal links, chunk retrieval, article scouting |
| `mechanical-mediawiki-sync` | Remote synchronization | Pull, redirects, conflicts, preview/apply plan binding, `baserevid`, `createonly`, lost responses, deletion lineage, reconciliation, operator closure |
| `mechanical-html-to-wikitext` | Import conversion | Source/target profiles, template projection, hash-bound caller-reported rendered-DOM receipt mechanics, stale-receipt rejection, source-fidelity controls, and typed CSS/JavaScript evidence |
| `mechanical-optional-companions` | Companion boundaries | Absent/present behavior and independent lifecycle authority |
| `mechanical-template-closure` | Template dependency closure | Capability-bound dependencies and explicit failure evidence |
| `catalog-structural-audit` | Structural analysis | References, duplicates, orphans, empty categories, template usage/implementation, authoring-contract search and planning |
| `mechanical-agent-authoring-workflow` | Agent-facing workflow | Adapter inspection, interview ledger/open items, brief-bound review, changeset preparation/acceptance, promotion, post-promotion validation |
| `mechanical-knowledge-workspace` | Local knowledge workspace | Resolved config, docs import/search/context/symbols, DB reset continuity, CSV/JSON cargo import, LSP config, Scribunto lint |
| `mechanical-mediawiki-reading` | Read-only MediaWiki | Text/title search, capability probe/sync/show, rendered-scope semantics, private-address export refusal |
| `mechanical-template-contracts` | Template engineering | Contract validation, scaffold plan binding/replay refusal, observed-contract capture, positive/negative DOM assertions, exact migration planning and collision refusal |

The local MediaWiki fixture observes exact HTTP parameters and mutable revision state. It does not
claim parity with every MediaWiki extension, authentication deployment, proxy, or production
failure mode. The campaign performs no live wiki writes.

## Prose campaigns

| Campaign | Purpose | Completion authority |
|---|---|---|
| `prose-dogfood` | Fast calibration across small authoring and controlled-review cases | External author/reviewer submissions plus held-out oracle evaluation |
| `complex-prose-stress` | Long-form source synthesis, quantitative and chronology discipline, governance boundaries, BLP safety, attribution, human-note contamination | External author and differently identified reviewer for both source-rich assignments |

Agent submissions are invalid unless they record exact provider, model, harness/version, invocation,
access envelope, and available execution metrics. Participant identity and execution metadata are
self-reported. Packet isolation, hashes, and distinct IDs do not prove who controlled an account.

Preparation records `prepared_coverage`, never `demonstrated_coverage`. A complex authoring case
demonstrates coverage only after an article decision, valid candidate and claim map, successful
mechanical observations, complete independent review, held-out oracle pass, and terminal acceptance.

## Out of scope

- Release archive construction and dispatch safety are build-system concerns, not Wikitest scores.
- Human publication acceptance is outside both mechanical and prose evaluation.
- A passing fictional packet does not establish quality for every topic, language, model, or wiki.
- Host repositories may add read-only supplemental scenarios; their site policy does not become
  public Wikitool authority.
