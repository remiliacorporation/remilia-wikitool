# Wikitest

Wikitest is Wikitool's executable dogfooding and editorial-evaluation laboratory. It is a separate
workspace binary and scenario authority, not a collection of assertions hidden inside Wikitool's
unit tests. It exercises the public `wikitool` executable and writes inspectable, hash-bound run
artifacts under `.wikitest/`.

## Authority boundaries

Wikitest has three lanes:

1. **Mechanical scenarios** prove public CLI, filesystem, structured-output, hash, timeout, and
   completeness facts in an isolated project.
2. **Knowledge scenarios** prove local content indexing, wikitext parsing, links, retrieval, and
   article-start behavior. A site may add a `host_read_only` catalog for its real local database.
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
target/debug/wikitest inspect .wikitest/runs/suite-.../receipt.json
```

Prepare the prose campaign or one assignment:

```text
target/debug/wikitest prose prepare-suite prose-dogfood
target/debug/wikitest prose prepare aster-index-authoring
```

The request names generated strict templates. An external author supplies an article, a
`wikitest.claim-map.v1`, and an author submission copied from the template:

```text
target/debug/wikitest prose submit-author RUN --submission author-submission.json
```

For authoring assignments, this creates `review/export/`. That directory contains only the
reviewer-visible assignment, exact candidate, claim map, source packet, review skills, generated
submission template, and mechanical observations. It excludes the author identity and any
controlled-case oracle. Run the reviewer from that export, then submit its exact JSON:

```text
target/debug/wikitest prose submit-review RUN --submission review-submission.json
target/debug/wikitest inspect RUN/receipt.json
```

An authoring run moves through `awaiting_author`, `awaiting_review`, and one of
`reviewed_accept`, `reviewed_revise`, or `reviewed_block`. The final state records the reviewer's
disposition; Wikitest does not compute readability from heuristics.

## Catalogs and adapters

The public catalog in this directory is project-generic. Sites combine it with a supplemental
catalog instead of adding site doctrine to the crate:

```text
wikitest --repo-root tools/wikitool \
  --catalog tools/wikitool/wikitest \
  --catalog wikitest \
  --host-root /path/to/wiki \
  suite site-dogfood --require-all
```

`host_read_only` commands are admitted by an explicit narrow allowlist and cannot override the
project root. A missing declared host database produces an honest skip unless `--require-all` is
used.

## Prose controls

The committed prose cases are fictional closed-world fixtures under `.invalid` URLs. They must
never be promoted to a real wiki. Their full assignment authority is retained under the run's
`holdout/` directory, while the review export receives a redacted assignment. Controlled review
oracles use stable protocol observations such as axis verdicts and disposition. Optional tag checks
are reserved for a declared canonical vocabulary, never private spellings a reviewer could not
know. Oracles are evaluated only after the review submission is fixed.

Every substantive mutation invalidates the relevant hash. `inspect` re-hashes the tool, manifests,
packets, inputs, candidates, submissions, process outputs, exports, child receipts, and coverage
sets. Retained receipts are evaluation evidence, not a claim that one run represents every wiki,
model, or article type.
