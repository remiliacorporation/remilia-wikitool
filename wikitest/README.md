# Wikitest

Wikitest is Wikitool's executable dogfooding and editorial-evaluation laboratory. It is a separate
workspace binary and scenario authority, not a collection of assertions hidden inside Wikitool's
unit tests. It exercises the public `wikitool` executable and writes inspectable, hash-bound run
artifacts under `.wikitest/`.

## Authority boundaries

Wikitest has three lanes:

1. **Mechanical scenarios** prove public CLI, filesystem, structured-output, hash, and
   completeness facts in an isolated project. A local MediaWiki fixture observes real HTTP request
   parameters and mutable revision state without touching a live wiki.
2. **Catalog scenarios** prove local content indexing, wikitext parsing, links, retrieval, and
   article-scout behavior. A site may add a `host_read_only` catalog for its real local database.
3. **Prose assignments** freeze source documents and the public writing/review skills, accept an
   external author's article and claim map, and create a blinded review export. Prose disposition
   comes only from a separately identified reviewer.

Lint success is never prose approval. A review receipt is never human publication acceptance.
Participant identities are self-reported; Wikitest proves what identity string was recorded and
that author and reviewer IDs differ, not who controlled either account.

## Commands

Build both binaries before running the laboratory:

```text
cargo build -p wikitool -p wikitest
```

Discover and validate the catalog, including suite membership and coverage closure:

```text
target/debug/wikitest list
target/debug/wikitest validate
```

Run the deterministic reusable suite:

```text
target/debug/wikitest suite core-dogfood --require-all
```

Every command that creates or advances a run re-opens the resulting receipt and replays all
retained data evidence, then verifies the SHA-256 identities of the Wikitest driver and evaluated
Wikitool binary at their recorded repository-relative or absolute locators. The run tree does not
copy either binary. Later inspection therefore requires an external immutable binary archive (or
restored build outputs) that places both exact binaries at those locators; a rebuilt binary makes
the historical receipt intentionally non-replayable in that checkout. Use `inspect
.wikitest/runs/suite-.../receipt.json` to detect data corruption only while that binary-locator
contract is satisfied.

An isolated command step may declare JSON scalar captures such as
`{"name":"PUSH_PLAN_ID","pointer":"/report/plan_id"}`. A later command can use the value as
`${PUSH_PLAN_ID}` in its argv or declared environment. Captures require complete, untruncated JSON
stdout and a nonblank string, number, or boolean at the declared pointer. Scenario validation rejects
forward references and redefinitions. The receipt binds each value to its source step, JSON pointer,
and retained stdout SHA-256; `inspect` re-derives that binding from the retained bytes. Dynamic
captures are prohibited in `host_read_only` scenarios so their command surface remains statically
auditable before execution.

Inspection distinguishes replay from authenticity. `evidence_replayed: true` means the assertions,
step closure, capability bindings, hashes, and current binary identities agree. The artifact set is
reported as `authenticity: unanchored` because an actor who can rewrite every local artifact can
also rewrite their hashes. Release claims must publish the suite receipt digest through an
independent immutable or signed channel.

Prepare the prose campaign or one assignment:

```text
target/debug/wikitest prose prepare-suite prose-dogfood
target/debug/wikitest prose prepare aster-index-authoring
```

The request names generated strict templates. Each prepare/submit command prints a participant
request path and a sibling `output/` directory. An external author supplies an article, a
`wikitest.claim-map.v1`, and an author submission copied from the template:

```text
target/debug/wikitest prose submit-author RUN --submission author-submission.json
```

Each participant export is created under the operating system's temporary directory, outside both
the source repository and the retained run/holdout tree. Give the participant only that printed
export root. Protected packet files are an exact allowlist; participant-created candidate, claim
map, or submission files belong under its `output/` directory. The reviewer export contains the
exact candidate, source/context packet, review skills, generated submission template, and
mechanical observations. It excludes the assignment artifact, internal case label and description,
author instructions, submission, claim map, identity, and controlled-case oracle. Reviewer-facing
case and run identifiers are opaque. Run the reviewer from that isolated export, then submit its
exact JSON:

```text
target/debug/wikitest prose submit-review RUN --submission review-submission.json
target/debug/wikitest inspect RUN/receipt.json
```

The external handoff root is an operational boundary, not the durable evidence store. Wikitest
requires it while the corresponding submission is pending. After submission, it may be removed;
the run retains the exact redacted request, packet, allowlisted inputs, outputs, and hashes needed
for archival replay. An authoring run moves through `awaiting_author`, `awaiting_review`, and one of
`reviewed_accept`, `reviewed_revise`, or `reviewed_block`. The final state records the reviewer's
disposition; Wikitest does not compute readability from heuristics.

`prepare-suite` records only `prepared_coverage`: it proves that packets exist for the declared
cases, not that an author or reviewer demonstrated those capabilities. After every child has a
review submission, evaluate the suite run:

```text
target/debug/wikitest prose evaluate-suite PROSE_SUITE_RUN
```

Coverage is recorded with an explicit status. Review assignments demonstrate coverage only with a
complete-scope, complete-source review whose held-out oracle passes. Authoring assignments also
require an `article` decision, retained candidate and valid claim map, no author/claim-map holds,
successful untruncated init/lint evidence, and an accepted terminal review. `missing_oracle`,
`review_incomplete`, `authoring_incomplete`, `authoring_rejected`, and `oracle_failed` are honest
non-demonstrations. Thus a hold, blocked/revise outcome, or merely completed review cannot credit
authoring quality. The evaluated suite passes only when every child has demonstrated its coverage.
Prose suites retain child runs beneath their own `runs/` tree, use root-relative locators, and bind
both the immutable preparation snapshot and the current evaluated child receipt. The suite data
tree may be relocated as a unit and inspection resolves no external child-run paths, but replay
still requires the exact external binaries at the receipt's recorded locators as described above.

`demonstrated_coverage` is bounded controlled-case protocol evidence: it proves the declared public
tags, axes, disposition, stage closure, and held-out expectations replay. Wikitest validates finding
structure and bindings, but does not independently decide whether free-text findings are semantically
entailed by the candidate and sources. Claims about reviewer quality or general source-fidelity
judgment require a separately adjudicated external reviewer campaign.

## Catalogs and adapters

The public catalog in this directory is project-generic. Sites combine it with a supplemental
catalog instead of adding site doctrine to the crate:

Catalog discovery deliberately does not descend into directories named `evidence`. A site may
retain a relocatable evaluated run tree there without its frozen `inputs/*.json` snapshots being
mistaken for a second live manifest authority. Evidence receipts remain inspectable by explicit
path and retain the external exact-binary locator contract described above.

```text
wikitest --repo-root tools/wikitool \
  --catalog tools/wikitool/wikitest \
  --catalog wikitest \
  --host-root /path/to/wiki \
  suite site-dogfood --require-all
```

`host_read_only` commands are admitted by an explicit narrow allowlist, but run against a temporary
snapshot of the recognized host `.wikitool`, content, template, and adapter surfaces. The source is
fingerprinted before and after execution; command-side SQLite/WAL or filesystem mutations affect
only the snapshot, while source drift fails the run. Every isolated command gets an empty local
`.env` plus runner-owned `--project-root`, `--data-dir`, and `--config`. Ambient `WIKITOOL_*`
values are scrubbed, and scenario declarations cannot restore path/API/runtime overrides. A missing
declared host database produces an honest skip unless `--require-all` is used.
Commands run in a dedicated Unix process group or Windows kill-on-close Job Object. Timeout and
normal completion terminate remaining descendants, and stdout/stderr capture has a bounded drain
deadline, so an inherited pipe cannot extend a declared process budget indefinitely.

The local MediaWiki fixture requires a login token, successful credentials, session cookie, and a
session-bound CSRF token before edits or deletes. Its no-write-retry cases apply a mutation and drop
the HTTP response; the scenario passes only when Wikitool reports the ambiguous failure and the
fixture observes exactly one request for each affected title. The edit case also binds the three
planned edits overall. Delete coverage binds previewed plan IDs into the exact apply commands,
exercises typed `missingtitle`, and independently asserts the complete delete-request count. A
visible delete marker is recovered through paginated `logevents`; a hidden marker remains
ambiguous, is closed with explicit operator provenance, blocks another write until a target-bound
full pull, and remains replayable through `mutation show` and `mutation list --all`.

## Prose controls

The committed prose cases are fictional closed-world fixtures under `.invalid` URLs. They must
never be promoted to a real wiki. Their full assignment authority is retained under the run's
`holdout/` directory. The author receives an oracle-redacted assignment, while the reviewer gets a
typed minimum projection containing only an opaque case ID, article brief, public finding-tag
vocabulary, and review axes. Reviewer packet/request digests do not commit to the internal catalog
ID, title, description, coverage labels, author-only constraints, claim map, or oracle; the full
authority digest is retained exclusively in evaluator evidence. Controlled review
oracles use stable protocol observations such as axis verdicts and disposition. Optional finding
tag checks must come from the assignment-declared canonical vocabulary copied into the blinded
request and submission template; only finding tags, not positive/control observation tags, are
scored. Accept/revise/block must also agree with the submission's P0-P3 finding severities. Oracles
are evaluated only after the review submission is fixed.

Every substantive mutation invalidates the relevant hash. `inspect` re-hashes the driver, tool,
manifests, packets, inputs, candidates, submissions, process outputs, exports, child receipts, and
coverage sets and re-derives deterministic assertion outcomes. Retained receipts are unanchored
evaluation evidence, not authenticated provenance or a claim that one run represents every wiki,
model, or article type. Rebuilding either binary intentionally makes the corresponding
identity stale; retain that run as historical evidence and create a new run for the new binary
instead of waiving the mismatch.

Wikitest is source-resident evaluation infrastructure. Release archives ship the end-user
Wikitool binary and agent pack, not this binary or its scenario catalogs; a standalone Wikitest
binary without the source catalog and skill inputs would be an incomplete evaluator.
