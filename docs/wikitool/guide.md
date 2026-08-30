# Wikitool Guide

Rust CLI for evidence-bound MediaWiki coauthoring: wiki-aware retrieval, real prose drafting through
external agents, mechanical wikitext work, exact-content human acceptance, revision-aware sync,
docs ingestion, and inspection utilities.

For command flags: `wikitool <command> --help` or `reference.md`.

## First Run

```bash
wikitool init --wiki-url https://wiki.example.org/ --api-url https://wiki.example.org/api.php
# If the project owns a site adapter, select and validate it explicitly:
wikitool init --adapter-path site-adapter/site-adapter.toml
wikitool pull --full --all
wikitool catalog warm --docs-mode missing
wikitool wiki capabilities sync
wikitool templates catalog build
wikitool catalog status --format json
```

Adapter paths are project-relative and canonically confined to the project root; external absolute
paths and symlink escapes are rejected so a copied project retains the exact policy it names.

## Session refresh

Run this at the start of an agentic editing session. Each step is explicit so an operator can stop
when local edits, target identity, network state, or derived catalogs need review:

```bash
wikitool status --modified --format json
wikitool diff --format json
wikitool pull --all
wikitool catalog warm --docs-mode missing
wikitool wiki capabilities sync
wikitool templates catalog build
wikitool catalog status --format json
```

For a first or deliberately rebuilt baseline, use `pull --full --all`. Do not use
`pull --overwrite-local` unless local edits should be discarded. Agent skills may present this
sequence as a recipe, but Wikitool does not hide these independently consequential operations
inside a macro command.

## How it works

- Pull/push use the MediaWiki API. Durable revision identity and base snapshots live in
  `.wikitool/sync/sync.sqlite3`; derived search, docs, and capability catalogs live in
  `.wikitool/data/wikitool.db`.
- The catalog DB is disposable. The sync store is not: diff, status, review, and push fail until a
  successful `pull --full --all` establishes global coverage. Scoped, category, incremental, and
  legacy-migrated state never grant global planning authority. The store is bound to the normalized
  configured API endpoint; changing wiki targets fails before planning or publication, including
  with `--force`, and requires preserving or moving the old store before establishing a fresh
  target baseline. `db reset` preserves or migrates legacy identity before deleting the catalog;
  the operator must then perform the authoritative pull explicitly.
- Authoring retrieval uses semantic page profiles, a DB-backed template/module contract graph, normalized source authorities, and bridged MediaWiki docs to narrow context for agents.
- `article scout` returns a typed retrieval-context surface plus local-fit signals. It does not embed an editorial prompt. Agent drafting is allowed through the writing skill, but model output and neighboring pages are not evidence. Use `--contract-query` when the subject and the wiki-contract lookup are different, for example a cheetah article whose contract search should ask for `species infobox taxonomy`.
- Token-efficient agent workflows should start from wikitool briefs and drill down only as needed. Prefer `article scout --view brief`, `catalog inspect chunks --view brief`, `templates show --view brief`, `catalog surface show --view brief`, and `review --view brief`; reserve `--view full` for cases where implementation bodies or complete capability arrays are explicitly needed.
- `article lint` / `article fix` are adapter-aware. `validate` is the lower-level index integrity check; use `--summary` for the global signal and scoped `--category`/`--title` slices for targeted investigation. `module lint` is the Lua/module lane.
- `article lint` / `article fix` accept repeated `--title`, repeated `--path`, `--titles-file`, and `--changed` for batch work.
- `review` is a structured mechanical pre-push report: status plan, changed article lint, validation summary, and push preview in one JSON result. It is distinct from the `prose-review` skill.
- A named human must accept the exact prose before promotion or any changed Main-namespace push.
  The transactional acceptance store is bound to content, target, and publication policy;
  `--force` cannot bypass it. Its editor label is a self-reported, unauthenticated claim.
- Push flows are preview-first. Preview an explicit title/path scope or deliberately pass `--all`,
  inspect the returned `plan_id`, then repeat the exact scope, summary, and policy flags with
  `--apply PLAN_ID`. Any target, content, revision, scope, or policy drift invalidates the plan.
  Existing-page writes are revision-bound and creates use create-only semantics. `--force`
  requires explicit user approval and never weakens the plan-ID or target-identity checks.

## Authoring workflow

```bash
wikitool article scout "Topic" --intent new --format json --view brief
wikitool article scout "Topic" --intent new --brief-path .wikitool/interviews/Title/20260601T172430Z.brief.md --format json --view brief
wikitool article scout "Cheetah" --contract-query "species infobox taxonomy" --format json --view brief
wikitool catalog contracts search "species infobox taxonomy" --format json
wikitool source wiki-search "Topic" --format json
wikitool source fetch "URL" --format rendered-html --output json
wikitool source mediawiki-templates "https://en.wikipedia.org/wiki/Article" --template "Template:Infobox" --format json
wikitool templates show "Template:Infobox person" --format json --view brief
wikitool templates examples "Template:Infobox person" --limit 2
wikitool templates closure "Template:Infobox person" --output .wikitool/template-closures/infobox-person.json --format json
wikitool adapter inspect --format json
wikitool wiki capabilities remote "https://www.mediawiki.org/wiki/Manual:Contents" --format json
# an agent, human, or both draft evidence-bound encyclopedic prose
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
wikitool article accept .wikitool/drafts/Title.wiki --title "Title" --human-editor "EDITOR" --prose-origin agent-draft --format json
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
# For a reviewed mechanical batch, freeze every changed item before one human decision:
wikitool article changeset prepare --changed --output .wikitool/review-changesets/mechanical.json --prose-origin mechanical-conversion-of-human-prose --format json
wikitool article changeset accept .wikitool/review-changesets/mechanical.json --human-editor "EDITOR" --warnings require-none --format json
wikitool article lint wiki_content/Main/Title.wiki --format json
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --summary "Draft review"
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --brief-path .wikitool/interviews/Title/20260601T172430Z.brief.md --format json --summary "Draft review"
wikitool article fix wiki_content/Main/Title.wiki --apply safe
wikitool catalog inspect references summary --title "Title" --format json
wikitool catalog inspect references duplicates --title "Title" --format json
wikitool validate --summary
wikitool review --format json --view brief --summary "Summary"
```

## Template engineering

The template catalog reconciles local implementation source, TemplateData, parameter aliases,
observed usage, examples, documentation pages, and directly invoked Scribunto modules. Use an
exact named dependency closure before changing or migrating a template family:

```bash
wikitool wiki capabilities sync
wikitool templates catalog build
wikitool templates closure \
  "Template:Infobox subject" \
  "Template:Preservation image" \
  --max-nodes 64 \
  --output .wikitool/template-closures/preservation.json \
  --format json
```

The closure follows runtime template transclusions, literal `#invoke`/TemplateStyles module pages,
and literal Scribunto `require('Module:...')` and `mw.loadData('Module:...')` calls. It honors
`onlyinclude`, excludes `noinclude` documentation examples, and records exact SHA-256 identities for
every local implementation/documentation file. MediaWiki magic words and parser functions come
from the stored siteinfo capability manifest; known Scribunto `strict` and `libraryUtil` loads are
reported as runtime-provided libraries. Genuine missing local templates/modules and dynamically
indeterminate Lua loads remain separate evidence rather than being suppressed or guessed. A
`#function` absent from the capability manifest is likewise unresolved, not assumed to exist.
`--max-nodes` fails instead of truncating a larger graph.

`template_dependency_closure_v2` adds `runtime_context` and `runtime_dependencies`; its edge list
also identifies runtime dependency kinds. File-write receipts are
`template_dependency_closure_write_v2` and add `runtime_dependency_count`. Because the capability
manifest is part of closure provenance and content hashing, consumers of v1 closures or receipts
must opt into v2 and regenerate their artifacts. Stored v1 capability manifests remain decodable,
but closure export requires refreshed magic-word data and tells the operator to run
`wiki capabilities sync` when it is absent. This export is mechanical evidence for design and
migration work; it does not decide aesthetics, create source-wiki clones, rewrite transclusions, or
establish that a template vocabulary is editorially appropriate.

Represent a reviewed target design with a strict `template_engineering_contract_v1` document. The
contract records the exact implementation body, TemplateData parameter contract, aliases, explicit
`migrate_from` names, declared dependencies, human-readable examples, and portable render
expectations. Optional documentation header and footer wikitext keep noinclude-only prose and
categories in conventional source order. A complete target-neutral example is provided at
`docs/wikitool/examples/template-contract.json`.

```bash
wikitool templates contract check docs/wikitool/examples/template-contract.json --format json
wikitool templates contract capture "Template:Existing" \
  --output .wikitool/template-contracts/existing.observed.json \
  --format json
wikitool templates contract render-check \
  docs/wikitool/examples/template-contract.json \
  --format json
wikitool templates scaffold docs/wikitool/examples/template-contract.json \
  --output templates/example/Template_Example_card.wiki \
  --format json
# Inspect the content/path/current-state-bound plan, then apply that exact plan:
wikitool templates scaffold docs/wikitool/examples/template-contract.json \
  --output templates/example/Template_Example_card.wiki \
  --apply PLAN_ID \
  --format json
```

Contract checks reject unknown implementation parameters, undeclared dependencies, malformed
examples, invalid render scopes, wrapper-control injection, and ambiguous parameter names. When a
catalog template is present, compatibility analysis classifies additions, requiredness and type
changes, explicit renames, removed aliases, and unmapped removals. The generated scaffold owns only
the mechanical `includeonly`/`noinclude`, documentation, usage, and TemplateData envelope. Capture
refuses overwrite and labels its output as an observed starter, not target design. Preview never writes;
apply is bound to the exact contract, output path, proposed content, and observed target hash.
Replacing different existing bytes also requires `--overwrite`. Render fixtures are exported as a
typed bundle; `templates contract render-check` executes each invocation through the configured
MediaWiki parser's read-only parse-text path and applies the existing scoped HTML gate. A fixture's
presence is not a claim that rendering has passed. Wikitool emits migration evidence but never
rewrites transclusions automatically.

## Encyclopedic coauthoring

An external agent may research and write a new article, substantial section, or source-backed
rewrite. The target is real encyclopedic prose: direct, neutral, specific, proportionate, coherent,
and useful to a reader. A named human editor owns the publication decision and accepts the exact
final bytes.

Start with `article scout --view brief`. Its `article_scout_v1` payload exposes local
state, context identities, retrieval coverage, available target-wiki mechanics, and explicit
ledger gaps. It deliberately carries no prompt prose or recommended editorial action.
`retrieval_ready` means current content/docs artifacts are available; it does not mean the topic is
researched or a draft can be published.

Before prose, build a claim-source map. Inspect the exact source behind load-bearing, sensitive,
interpretive, quoted, and surprising claims. Treat an exact local page as coverage to audit and a
neighboring page as local fit context, not independent verification. Follow the canonical
`wiki-writing` skill, write the factual body before the lead, then run the independent
`prose-review` skill. Delete generic padding rather than editing it into smoother padding.

For new articles, substantial revisions, niche history, and unclear framing, use the packaged
interview skill. Read supplied materials before narrowing questions. Ask only questions that can
improve the article object, claim-source map, terminology, chronology, emphasis, exclusions, or
risk. The human does not need to pre-write the article.

Reusable interview distillations should be saved as:

```text
.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md
```

The brief is an editorial ledger, not automatic independent evidence, proof, or acceptance.
Keep firsthand knowledge, human notes, source leads, and inspected sources distinct. A project's
site adapter may describe how its own records and first-party sources should be used, but this does
not waive claim-level provenance or source-role judgment. Define adjacent subjects on their own
terms and give relationships only the weight supported by the evidence. Use stable open-item IDs
for unresolved source work, do-not-assert holds, and negative evidence that need tracking through
research and review.

Use the Rust interview ledger commands for deterministic paths, starter files, validation, compact
handoff summaries, and ledger audits:

```bash
wikitool interview init "Topic" --intent new --format json
wikitool interview validate .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool interview show .wikitool/interviews/Title/20260601T172430Z.brief.md --view brief --format json
wikitool interview open-item add .wikitool/interviews/Title/20260601T172430Z.brief.md --kind rejected-source --text "Candidate source did not support the claimed date."
wikitool interview open-item update .wikitool/interviews/Title/20260601T172430Z.brief.md --item-id OI-20260601T172430Z --status resolved
wikitool interview open-item list .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool interview audit --view brief --format json
```

The conversational interview loop belongs in the `wiki-interview` skill. The CLI does not infer source
support from user prose, call a model, or decide that an interview is editorially sufficient; it
validates structured metadata, required sections, sidecars, typed open-items JSONL records,
negative-evidence counts, and freshness.

Pass the validated brief to `article scout --brief-path` or `review --brief-path` when
the interview should shape research planning or gate review. These integrations surface explicit
brief metadata, do-not-assert holds, open research items, and negative-evidence counts; they do not
treat mechanical validation as editorial acceptance or factual support.

For custom content features, inspect the active site adapter and deployed target contract rather
than assuming raw HTML, JavaScript, a parser tag, or a module is portable. Use `adapter inspect`,
`wiki capabilities show`, `catalog surface`, `catalog contracts`, `templates show`, and `article lint` to verify the actual
mechanism.

## Sync

The 0.7 sync/write surface is scoped to MediaWiki's `first-letter` title-case policy. Do not use it
against a case-sensitive namespace until Wikitool carries the siteinfo case policy into its durable
title identity; silently collapsing case-distinct pages would be an authority error.

```bash
wikitool pull                          # latest content
wikitool pull --all                    # session refresh across articles/templates/categories
wikitool pull --full --all             # full refresh
wikitool pull --templates              # templates only
wikitool status                        # sync-aware status summary
wikitool status --modified --format json
wikitool status --conflicts --title "Title"
wikitool diff                          # review change set
wikitool diff --content --title "Title"
wikitool review --format json --view brief --summary "x"
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --summary "x"
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --brief-path .wikitool/interviews/Title/20260601T172430Z.brief.md --format json --summary "x"
wikitool push --title "Title" --summary "x" --format json  # preview; inspect report.plan_id
wikitool push --title "Title" --summary "x" --apply "PLAN_ID"
wikitool push --all --summary "x" --format json             # deliberate all-change preview
wikitool wiki render-check "Consumer title" --scope-class card --expect-scopes 1 --require-interactive-link --require-href-contains "/File:" --require-link-class mw-file-description --require-page-image Example_Preview.png --format json
wikitool delete "Title" --reason "x" --format json  # preview; inspect plan.plan_id
wikitool delete "Title" --reason "x" --apply "PLAN_ID"
wikitool mutation list --format json
wikitool mutation show edit 42 --format json
wikitool mutation reconcile edit 42 --format json
```

Inspect `remote_exists`, `remote_revision_id`, and conflict details in the preview output. A locally
modified page that was deleted remotely is not an unconstrained update: it is blocked as a
conflict, and `--force` can only recreate it through MediaWiki `createonly`. Delete pushes perform
another revision lookup immediately before mutation, and `--force` does not waive a mismatch found
by that check. MediaWiki delete requests have no `baserevid`-style condition, so a revision landing
after the lookup but before the delete request remains a time-of-check/time-of-use window. An
already absent page is reconciled locally without sending a delete request.

Every remote edit or delete first records a target-bound intent in the durable sync store. A
successful response is reconciled against current remote state before local sync authority
advances. Wikitool never retries an ambiguous write. If a request loses its response or a process
stops mid-transition, inspect the mutation rather than rerunning the original command:

```bash
wikitool mutation list
wikitool mutation show delete 42 --format json
wikitool mutation reconcile delete 42 --format json
```

Reconciliation exits nonzero while the outcome remains ambiguous or requires intervention. If the
remote outcome cannot be proved, an operator may preserve the evidence and close the uncertainty
without claiming success or failure:

```bash
wikitool mutation close delete 42 \
  --actor "OPERATOR" --reason "remote lineage is unavailable" --confirm --format json
wikitool mutation show delete 42 --format json
wikitool pull --full --all
```

Closure records its own durable receipt, invalidates the title's previous sync baseline, and keeps
future writes blocked until a fresh target-bound pull observes the page or an authoritative full
pull proves global absence. When a present remote page conflicts with a locally drifted source,
that recovery pull also requires a deliberate `--overwrite-local`. Delete plans bind the target,
title, observed revision, reason, and exact local backup/removal policy; applying different inputs
is a plan mismatch.

Run `wiki render-check` after pushing templates, Cargo queries, or other dynamic
rendering changes. It checks production parser output rather than source text,
rejects parser-error markup and literal rendered wikilinks by default, can
assert an exact number of scoped components, and distinguishes interactive
anchors from crawler-only file-source links. Repeat the check for each consumer
shape whose click or link behavior is part of the cutover contract. Use
`--require-link-class mw-file-description` to enforce native MediaViewer links
rather than merely accepting a custom anchor that happens to target a file page.
Use `--require-page-image FILE` when mouseover previews, search, or other
PageImages consumers must select an exact representative image; this adds one
bounded `prop=pageimages` request and reports the selected thumbnail URL.

## Catalog and retrieval

```bash
wikitool catalog build                # content index only
wikitool catalog warm --docs-mode missing  # index + adapter-selected docs readiness
wikitool catalog status --format json
wikitool article scout "Topic" --intent new --format json --view brief
wikitool interview init "Topic" --intent new --format json
wikitool interview validate .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool interview show .wikitool/interviews/Title/20260601T172430Z.brief.md --view brief --format json
wikitool interview open-item add .wikitool/interviews/Title/20260601T172430Z.brief.md --kind missing-source --text "Need a citable source for the launch date."
wikitool interview open-item update .wikitool/interviews/Title/20260601T172430Z.brief.md --item-id OI-20260601T172430Z --status resolved
wikitool interview open-item list .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool interview audit --view brief --format json
wikitool article scout "Topic" --brief-path .wikitool/interviews/Title/20260601T172430Z.brief.md --format json --view brief
wikitool catalog contracts plan "Topic" --contract-query "subject type infobox" --format json
wikitool catalog inspect stats
wikitool catalog inspect chunks "Title" --query "aspect" --limit 6 --token-budget 480
wikitool catalog inspect chunks --across-pages --query "topic" --max-pages 8 --token-budget 1200 --format json --diversify
wikitool catalog inspect references summary --format json
wikitool catalog inspect references list --title "Title" --domain example.org --format json
wikitool catalog inspect references duplicates --all --identifier-key doi --format json
wikitool catalog inspect backlinks "Title"
wikitool catalog inspect orphans
wikitool catalog inspect empty-categories
```

## Research

```bash
wikitool source wiki-search "topic" --format json
wikitool source fetch "URL" --format rendered-html --output json
wikitool source session import "URL" --cookies - --user-agent "UA" --ttl-seconds 1800 --format json
wikitool source session list --format json
wikitool source session show example.com --format json
wikitool source session clear example.com --format json
wikitool source session prune --format json
wikitool source mediawiki-templates "https://en.wikipedia.org/wiki/Article" --template "Template:Infobox" --format json
wikitool source mediawiki-templates "https://en.wikipedia.org/wiki/Article" --refresh --format json
wikitool source discover "URL" --format json
wikitool export "URL" --subpages --combined --limit 25
wikitool export --urls-file sources.txt --output-dir wikitool_exports/sources --format markdown
```

`source wiki-search` searches the configured target wiki API, not the open web. For arbitrary subjects, use the agent's normal web-search capability to choose source URLs, then use `source fetch`, `source discover`, `source mediawiki-templates`, and `export` for source extraction and provenance. `source fetch --output json` returns a `status` envelope. When `status` is `"error"`, inspect `error.kind`, `error.attempts`, `error.challenge_handoffs`, and `error.discovery`; access challenges and HTTP failures are explicit source-access failures, not citable source content. `source discover` is the same machine-surface discovery pass as a standalone command.

When a source returns a browser access challenge, `error.challenge_handoffs` gives the detected vendor, domain, expected cookie names when known, the user-agent that wikitool used, exact `suggested_argv`, and a display `suggested_command`. Wikitool does not solve, bypass, or disguise Cloudflare, Anubis, DataDome, AWS WAF, or similar challenges. If the user has lawful access, ask them to open the URL in a real browser, solve the challenge, then paste source-issued cookies into `wikitool source session import URL --cookies -`. Cookie input must come from stdin or an existing regular, non-symlink file; that payload may use Netscape `cookies.txt`, JSON, or raw `Cookie` header syntax. Never place cookie values directly in `--cookies`: literal values are rejected without being echoed in diagnostics. Imported sessions live under `.wikitool/source/sessions/`, list/show output masks cookie values, and matching sessions are used automatically by `source fetch`, MediaWiki template inspection, and `export`. Wikitool secures and verifies the empty staging file before writing cookie bytes: Windows uses a protected current-user-only DACL, while Unix uses current-effective-user ownership with directory mode `0700` and file mode `0600`. Uncertainty fails closed. Retry source fetches with `--refresh` after importing a session. Use `source session clear DOMAIN` to remove a session and `source session prune` to remove expired sessions.

`source mediawiki-templates URL` inspects the live API surface of a source MediaWiki page. Use it when an arbitrary source wiki, such as Wikipedia, has templates/modules that are relevant to understanding the source article but are not part of the current target wiki catalog. The report preserves total transclusion counts, returns a capped inventory, shows selected invocations, fetches selected template pages, and includes TemplateData when the source wiki exposes it. Results are cached under the source cache; use `--refresh` when live freshness matters and `--no-cache` for a one-off bypass. Treat these as source-wiki contracts only; run local `catalog contracts`, `templates show`, and `article lint` before using any template on the target wiki.

`wiki capabilities remote URL` probes a target MediaWiki URL without storing it as the local project target. It reports the remote wiki's live capability surface only: extensions, parser tags, parser functions, namespaces, and related API features. It does not provide adapter policy or a template/module catalog and does not make source-wiki templates portable.

`source fetch` and `export` accept MediaWiki short URLs, `index.php?title=` URLs, and subdirectory installs. `export` defaults to markdown: MediaWiki URLs are fetched as wikitext and rendered into agent-readable markdown, while arbitrary web pages use the readable-document extractor and include source/extraction metadata in frontmatter. Use `--output-dir DIR` with a single URL to write a title-based markdown or wikitext file under that directory. Use `--subpages --limit N` to bound large MediaWiki tree exports. Use `--urls-file PATH --output-dir PATH --format markdown` to create off-wiki source packs; blank lines and `#` comments in the URL file are ignored, and `_index.md` records successes and failures. Wikitext export requires a recognizable MediaWiki URL; blocked arbitrary sources fail explicitly instead of producing challenge-page content.

`review --draft-path PATH --title TITLE` runs article lint and global readiness on an off-wiki draft
under `.wikitool/drafts/`. It skips the push preview because the draft is not syncable yet. Its
`next_steps` require direct lint/fix, exact-content human acceptance, promotion, and scoped
post-promotion review. An agent must never self-attest as the human editor.

For direct draft iteration, `article lint` and `article fix` accept a single state-draft path plus
`--title TITLE`. This keeps title-sensitive linting and safe fixes available before the draft is
promoted into `wiki_content/`.

```bash
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
wikitool article accept .wikitool/drafts/Title.wiki --title "Title" --human-editor "EDITOR" --prose-origin agent-draft --format json
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
```

Acceptance records a self-reported editor claim, acceptance timestamp, truthful prose origin, lint
summary, warning decision, exact-content SHA-256, normalized MediaWiki API endpoint, and the full
SHA-256 identity of the active site-adapter policy and declared guidance. It is an auditable
transactional authorization and workflow interlock, not identity authentication or a prose-quality
certificate. Any later content change, target change, or publication-policy change invalidates the
row. Legacy JSON ledgers are detected as historical evidence but are never read as a parallel
authority path. Use `agent-draft`,
`collaborative-draft`, `human-draft`, `human-revision`,
`mechanical-conversion-of-human-prose`, or `human-reviewed-legacy` as accurate.

Use `article changeset prepare` when one named human is reviewing a coherent group of existing
Main-namespace articles. Preparation writes a strict JSON manifest containing every path, title,
content hash, prose origin, and lint finding. Acceptance re-reads and re-lints every item, refuses
the whole decision if any byte or lint evidence changed, then commits the decision and every member
authorization in one SQLite transaction. `--warnings require-none` is the default; use `--warnings
accept` only after the named human has reviewed every warning in the manifest. A later byte change
invalidates only that article's authorization; it never inherits acceptance from the rest of the
changeset.

Push previews and apply reports include verified acceptance provenance for prose pages: content
SHA-256, acceptance timestamp, prose origin, `self_reported_unverified` identity assurance, warning
decision, target/policy authority, and batch identity when present. The self-reported editor name
remains local and is not appended to the MediaWiki edit summary.

## Editor integration

```bash
wikitool lsp generate-config
wikitool lsp status
wikitool lsp info
```

## Docs

Catalog and docs commands default to the configured site adapter's `docs_profile`.
An explicit `--docs-profile`, docs profile argument, or `--profile` wins; projects
without an adapter use `mw-1.44-authoring`.
Schema-v1 precomposed docs bundles are intentionally unprofiled. Their generic
corpora remain available alongside any selected docs profile, while corpora with
different non-empty profiles remain isolated from one another.

```bash
wikitool docs import-profile
wikitool docs import --bundle ./ai/docs-bundle-v1.json
wikitool docs search "topic"
wikitool docs context "Extension" --format json
wikitool docs symbols "$wg"
wikitool docs list
wikitool docs update
```

## Adapter, capabilities, and templates

```bash
wikitool templates show "Template:Cite web" --format json --view brief
wikitool templates examples "Template:Cite web" --limit 2
wikitool templates catalog build
wikitool wiki capabilities sync --format json
wikitool adapter inspect --format json
wikitool catalog surface show --format json --view brief
```

## Diagnostics

```bash
wikitool status
wikitool lsp status
wikitool lsp info
wikitool db stats
wikitool module lint --format text
```

## Release packaging

These maintainer commands are available from source-checkout builds only when the maintainer surface
is explicitly enabled. Packaged end-user binaries do not include them, and they remain hidden from
`wikitool --help` output and the generated reference.

```bash
cargo run --package wikitool --features maintainer -- release build-matrix --targets x86_64-pc-windows-msvc,x86_64-unknown-linux-gnu,x86_64-apple-darwin
cargo run --package wikitool --features maintainer -- release build-matrix --targets x86_64-unknown-linux-gnu --unversioned-names
cargo run --package wikitool --features maintainer -- release build-matrix --targets x86_64-unknown-linux-gnu --host-project-root <PATH>
```

The public guidance and canonical skills always remain target-neutral. With
`--host-project-root`, packaging accepts only the host's `wikitool_adapter/` and places it under
`site_adapter/project/`; it never replaces `CLAUDE.md`, `AGENTS.md`, `.claude/`, or
`codex_skills/`. A missing typed `site-adapter.toml` fails the build.

## Troubleshooting

If local state drifts or schema changes:

```bash
wikitool db reset --yes               # preserves or migrates durable sync identity
wikitool pull --full --all
wikitool catalog warm --docs-mode refresh
wikitool wiki capabilities sync
wikitool templates catalog build
```

If push/delete writes fail, verify `WIKITOOL_BOT_USER` and `WIKITOOL_BOT_PASS` in the selected
project root `.env`. Wikitool does not search ancestor `.env` files, and explicit process variables
take precedence over the project file.

Starting in v0.2.0, pre-manifest databases are treated as incompatible. The supported path is reset, repull, rebuild.
