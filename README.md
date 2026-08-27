# Wikitool

Wikitool is a general-purpose, agent-native CLI for MediaWiki. It pulls and pushes revision-bound
wikitext, builds a disposable local knowledge index, exposes templates and capabilities, fetches
research material, applies deterministic checks, and packages substantive agent skills for
encyclopedic writing and review.

The binary does not contain an LLM and does not decide whether prose is good. Its Rust core owns
mechanical truth and safe state transitions. Agent skills own research-to-claim reasoning,
prose authoring, source-fidelity review, and reader judgment. A project-owned site adapter supplies
target-specific machine policy and supplemental guidance.

## Install

Download a release archive for your platform, verify it against `SHA256SUMS.txt`, and unpack it:

```text
wikitool(.exe)        end-user binary
README.md             this file
CLAUDE.md AGENTS.md   identical agent integration brief
.claude/              Claude harness adapters
codex_skills/         canonical substantive agent skills
integration/          layer and adapter contracts
site_adapter/         generic adapter and optional project supplement
docs/wikitool/        operator guide and generated command reference
contextmink/          separately versioned transcript guard
manifest.json LICENSE*
```

Put `wikitool` on `PATH`, or run it from the unpacked directory.

## Configure a wiki

Wikitool has no built-in target wiki. Initialize a project with an explicit MediaWiki endpoint:

```bash
wikitool init \
  --wiki-url https://wiki.example.org/ \
  --api-url https://wiki.example.org/api.php
wikitool workflow session-refresh
```

This creates `.wikitool/config.toml`, pulls content, builds the local index, and discovers the
wiki's capability surface. Read-only work needs no credentials. Writes use bot-password credentials
from the project-root `.env`:

```bash
WIKITOOL_BOT_USER=Username@BotName
WIKITOOL_BOT_PASS=your-bot-password
```

Set `wiki.mark_edits_as_bot = true` only when edits should carry MediaWiki's bot flag. Human review
or an acceptance-ledger entry does not imply bot or non-bot transport policy.

## Site adapters

Without an adapter, Wikitool uses a conservative embedded `mediawiki-generic` profile. A project
can opt into typed policy:

```bash
wikitool init --adapter-path site-adapter/profile.toml
```

This validates the complete declared adapter bundle before recording its path. The equivalent
configuration is:

```toml
[adapter]
path = "site-adapter/profile.toml"
```

Start from `config/generic-site-adapter.toml`. Adapters may declare authoring mechanics, citation
templates and source-review host rules, template preferences, deterministic lint configuration,
extension contracts, and supplemental Markdown documents. Unknown fields fail closed. A source
matcher is a review signal, not a universal source ban. Adapter Markdown is hashed and exposed to
agents but is never interpreted as machine policy. Release packaging strictly parses the adapter
and ships only `profile.toml` plus guidance files declared there; undeclared neighboring files are
excluded.

## Agent workflow

The packaged skills divide responsibility deliberately:

- `wikitool-operator` — retrieval, templates, lint/fix, ledgers, sync, and diagnostics.
- `wiki-writing` — source inspection, claim-source maps, human-notes handling, and real
  encyclopedic drafting.
- `prose-review` — independent source-fidelity, due-weight, BLP, structure, and reader-value review.
- `wiki-interview` — open-ended human intake into a neutral, auditable ledger.

A typical authoring lane is:

```bash
wikitool knowledge article-start "Topic" --intent new --format json --view brief
wikitool research fetch "https://source.example/work" --format rendered-html --output json
# use wiki-writing to build a claim-source map and draft the factual body, then the lead
# use prose-review in a fresh context when possible
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
# a named human reads and accepts the exact final prose
wikitool article accept .wikitool/drafts/Title.wiki --title "Title" \
  --human-editor "EDITOR" --prose-origin agent-draft --format json
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool review --format json --view brief --summary "Review Title"
wikitool push --dry-run --summary "Update Title"
```

The acceptance ledger binds a decision to the exact SHA-256 content. The editor label is a
self-reported, unauthenticated claim; it is not identity proof, and an agent must never
self-attest. Any prose change invalidates the entry.

## Core capabilities

- `pull`, `status`, `diff`, `review`, `push` — revision-aware synchronization. Existing edits use
  `baserevid`; creates and explicit remote-deletion recreations use `createonly`; deletes recheck
  revision identity immediately before mutation; mutating requests are not generically retried.
- `knowledge build|status|article-start|inspect` — bounded local retrieval with explicit readiness
  and evidence identities.
- `templates`, `wiki capabilities|profile|surface`, `docs` — target capability and contract
  discovery.
- `research wiki-search|fetch|discover|session|mediawiki-templates` — source acquisition with a
  shared outbound HTTP policy.
- `article lint|fix`, `validate`, `module lint`, `wiki render-check` — deterministic mechanical
  checks. Passing them is not an editorial quality verdict.
- `knowledge interview` — stable brief paths, neutral sections, structured open items, freshness,
  and validation; conversational judgment stays in the skill.

Every command has `--help`. `docs/wikitool/reference.md` is generated from the CLI and is the flag
authority.

## Runtime state

```text
project-root/
  .env
  .wikitool/config.toml
  .wikitool/data/wikitool.db       derived and disposable
  .wikitool/drafts/
  .wikitool/acceptance-ledger/
  site-adapter/profile.toml        optional, project-owned
  wiki_content/                    synchronized wikitext
  templates/                       template and module sources
```

Reset stale derived state with:

```bash
wikitool db reset --yes
wikitool workflow full-refresh
```

## Contextmink boundary

Release bundles include a version-pinned upstream Contextmink pack. Contextmink owns its binary,
templates, project setup, and install receipt; Wikitool does not carry a fork or second installer.

## Wikitest evaluation

Source checkouts include `wikitest`, a separate workspace binary that exercises the public
`wikitool` executable rather than reaching through crate internals. It owns strict catalogs for
mechanical workflows, synthetic knowledge-index retrieval, and externally authored and reviewed
prose assignments:

```bash
cargo build -p wikitool -p wikitest
target/debug/wikitest validate
target/debug/wikitest suite core-dogfood --require-all
target/debug/wikitest prose prepare-suite prose-dogfood
```

Run-producing commands re-open and verify their hash-bound receipts—including the exact Wikitest
driver and evaluated Wikitool binaries—before returning success. A host wiki can add a read-only
supplemental catalog for its real local database without moving site doctrine into Wikitest or
Wikitool. Wikitest and its catalogs are development/release-evaluation substrate and are
intentionally absent from end-user release archives; see `wikitest/README.md`.

## Build from source

```bash
cargo build --workspace --release
cargo test --workspace --all-targets
```

Reference generation, docs audit, and release packaging are maintainer-only:

```bash
cargo run --package wikitool --features maintainer -- docs generate-reference
cargo run --package wikitool --features maintainer -- docs audit
```

## Documentation

| File | Role |
|---|---|
| `docs/wikitool/guide.md` | Operator manual |
| `docs/wikitool/reference.md` | Generated command reference |
| `docs/wikitool/architecture.md` | Layering and authority boundaries |
| `ai-pack/integration/` | Generic agent and site-adapter contracts |
| `ai-pack/codex_skills/` | Canonical editorial and operator skills |
| `wikitest/README.md` | Executable mechanical, knowledge, and prose evaluation laboratory |
| `VERSIONING.md` / `CHANGELOG.md` | Release policy and history |

## License

AGPL-3.0-only, with supplementary terms in `LICENSE-SSL` and `LICENSE-VPL`.
