# Wikitool

Wikitool is a general-purpose, agent-native CLI for MediaWiki. It pulls and pushes revision-bound
wikitext, builds a disposable local content catalog, exposes templates and capabilities, captures
source material, applies deterministic checks, and packages substantive agent skills for
encyclopedic writing and review.

The binary does not contain an LLM and does not decide whether prose is good. Its Rust core owns
mechanical truth and safe state transitions. Agent skills own research-to-claim reasoning,
prose authoring, source-fidelity review, and reader judgment. A selected site adapter supplies
target-specific machine policy and supplemental guidance.

## Install

Download a release archive for your platform, verify it against `SHA256SUMS.txt`, and unpack it:

```text
wikitool(.exe)        end-user binary
README.md             this file
agent/                 deterministic agent pack, manifest, skills, and contracts
site_adapters/        built-in adapter catalog and optional project supplement
docs/wikitool/        operator guide and generated command reference
contextmink/          separately versioned transcript guard
papertiger/           optional separately versioned planning companion
release-companions.json  machine-readable optional-companion capabilities
LICENSE*
```

Put `wikitool` on `PATH`, or run it from the unpacked directory.

Validate and install the skills into an agent project from the unpacked release:

```bash
wikitool agent inspect
wikitool agent setup-project /path/to/project --target auto
```

`auto` follows existing `.agents` and `.claude` project markers and defaults to
`.agents/skills/` for an unmarked project. Setup installs exact, receipt-owned copies without
editing root instruction files. See `wikitool agent --help` for explicit targets, dry-run,
inspection, and safe uninstall.

macOS GitHub archives are explicitly unsigned. They cannot make Gatekeeper trust themselves; the
packaged agent procedure verifies the published checksum before removing quarantine from only the
exact executables the user approves. See [macOS Gatekeeper and release
trust](docs/wikitool/macos-gatekeeper.md).

## Configure a wiki

Wikitool has no built-in target wiki. Initialize a project with an explicit MediaWiki endpoint:

```bash
wikitool init \
  --wiki-url https://wiki.example.org/ \
  --api-url https://wiki.example.org/api.php
wikitool pull --full --all
wikitool catalog warm --docs-mode missing
wikitool wiki capabilities sync
wikitool templates catalog build
```

This creates `.wikitool/config.toml`, establishes an authoritative all-namespace sync baseline,
builds the local index, and discovers the wiki's capability surface. Read-only work needs no
credentials. Writes use bot-password credentials from the selected project-root `.env` (ancestor
`.env` files are ignored; explicit process variables take precedence):

```bash
WIKITOOL_BOT_USER=Username@BotName
WIKITOOL_BOT_PASS=your-bot-password
```

The 0.7 sync authority supports MediaWiki's usual `first-letter` title-case policy. Do not use its
write/sync surface on a case-sensitive namespace yet: title identity would not be safe until the
namespace case policy is carried from siteinfo into every local and remote authority key.

Set `wiki.mark_edits_as_bot = true` only when edits should carry MediaWiki's bot flag. Human review
or a publication-acceptance authorization does not imply bot or non-bot transport policy.

## Site adapters

Without a selected adapter, Wikitool uses a conservative embedded `mediawiki-generic` adapter.
Release archives also include reusable generic and Remilia Wiki adapter directories under
`site_adapters/`; their presence never selects a wiki, endpoint, or policy. Copy the desired
directory into the project before selecting its typed policy:

```bash
cp -R /path/to/unpacked-wikitool/site_adapters/remilia-wiki site-adapter
wikitool init --adapter-path site-adapter/site-adapter.toml
```

This validates the complete declared adapter bundle before recording its path. The equivalent
configuration is:

```toml
[adapter]
path = "site-adapter/site-adapter.toml"
```

Start from the strict `site_adapter_v2` template in
`site_adapters/generic/site-adapter.toml`. Adapters may declare authoring mechanics, citation
templates and source-review host rules, template preferences, deterministic lint configuration,
extension contracts, and supplemental Markdown documents. Unknown fields fail closed. A source
matcher is a review signal, not a universal source ban. Adapter Markdown is hashed and exposed to
agents but is never interpreted as machine policy. The built-in catalog is versioned and shipped
with Wikitool. An optional `--host-project-root` supplement is strictly parsed and ships only its
`site-adapter.toml` plus declared guidance files; undeclared neighboring files are excluded.

## Agent workflow

The packaged skills divide responsibility deliberately:

- `wikitool-operator` — retrieval, templates, lint/fix, ledgers, sync, and diagnostics.
- `wiki-writing` — source inspection, claim-source maps, human-notes handling, and real
  encyclopedic drafting.
- `prose-review` — independent source-fidelity, due-weight, BLP, structure, and reader-value review.
- `wiki-interview` — open-ended human intake into a neutral, auditable ledger.

A typical authoring lane is:

```bash
wikitool article scout "Topic" --intent new --format json --view brief
wikitool source fetch "https://source.example/work" --format rendered-html --output json
# use wiki-writing to build a claim-source map and draft the factual body, then the lead
# use prose-review in a fresh context when possible
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
# a named human reads and accepts the exact final prose
wikitool article accept .wikitool/drafts/Title.wiki --title "Title" \
  --human-editor "EDITOR" --prose-origin agent-draft --format json
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool review --format json --view brief --summary "Review Title"
wikitool push --path wiki_content/Main/Title.wiki --summary "Update Title" --format json
# inspect report.plan_id, then apply those exact target/content/revision bytes
wikitool push --path wiki_content/Main/Title.wiki --summary "Update Title" --apply "PLAN_ID"
```

The durable acceptance store binds a decision to the exact SHA-256 content, target wiki, and active
adapter-policy identity. The editor label is a self-reported, unauthenticated claim; it is not identity
proof, and an agent must never self-attest. Any prose, target, or policy change invalidates the
authorization. A multi-article decision and every member authorization commit in one SQLite
transaction.

## Core capabilities

- `pull`, `status`, `diff`, `review`, `push`, `delete`, and `mutation` — revision-aware
  synchronization with target-bound plans and durable remote-mutation receipts. Push and standalone
  delete preview by default and apply only the exact returned plan ID. Existing edits use
  `baserevid`; creates and explicit remote-deletion recreations use `createonly`; deletes recheck
  revision identity immediately before mutation. MediaWiki exposes no `baserevid`-equivalent for
  delete, so this narrows but cannot eliminate the interval between that check and the delete
  request. Mutating requests are not generically retried.
- `catalog build|status|inspect` and `article scout` — bounded local retrieval with explicit readiness
  and evidence identities.
- `adapter inspect`, `wiki capabilities`, `templates`, `catalog surface`, `docs` — explicit
  adapter policy, live capability, and derived contract discovery.
- `source wiki-search|fetch|discover|session|mediawiki-templates` — source acquisition with a
  shared outbound HTTP policy.
- `article lint|fix`, `validate`, `module lint`, `wiki render-check` — deterministic mechanical
  checks. Passing them is not an editorial quality verdict.
- `interview` — stable brief paths, neutral sections, structured open items, freshness,
  and validation; conversational judgment stays in the skill.

Every command has `--help`. `docs/wikitool/reference.md` is generated from the CLI and is the flag
authority.

If a network or process failure leaves a write outcome uncertain, do not repeat the write. Use
`wikitool mutation list`, `mutation show`, and `mutation reconcile` to inspect the target-bound
receipt and remote lineage. If remote truth cannot be proved, `mutation close ... --confirm`
records an operator-attributed unresolved closure without inventing an outcome, invalidates that
title's sync authority, and requires `wikitool pull --full --all` before another write. A present
remote page whose local source has drifted may additionally require an explicit
`--overwrite-local` pull. Closure provenance remains visible through `mutation show` and
`mutation list --all`.

## Runtime state

```text
project-root/
  .env
  .wikitool/config.toml
  .wikitool/data/wikitool.db       derived and disposable
  .wikitool/sync/sync.sqlite3      durable target-bound revision identity and base snapshots
  .wikitool/acceptance/
    acceptance.sqlite3             durable transactional publication authority
  .wikitool/drafts/
  site-adapter/site-adapter.toml   optional, project-owned
  wiki_content/                    synchronized wikitext
  templates/                       template and module sources
```

Reset stale derived state without deleting sync or publication-acceptance authority with:

```bash
wikitool db reset --yes
wikitool pull --full --all
wikitool catalog warm --docs-mode refresh
wikitool wiki capabilities sync
wikitool templates catalog build
```

## Contextmink boundary

Release bundles include a version-pinned upstream Contextmink pack. Contextmink owns its binary,
templates, project setup, and install receipt; Wikitool does not carry a fork or second installer.

## Papertiger boundary

Release bundles include a version-pinned upstream Papertiger pack as an optional authoring
companion. Wikitool works without a project-local Papertiger installation and never creates or
mutates Papertiger's task authority. To opt a project in, preview and then apply Papertiger's own
receipt-backed setup; that setup installs Papertiger's canonical agent skill and contract:

```bash
./papertiger/papertiger setup-project /path/to/project --skill-target both --dry-run --json
./papertiger/papertiger setup-project /path/to/project --skill-target both --json
```

Use `papertiger.exe` on Windows. Re-running setup performs an upgrade while preserving the selected
authority path and skill target. `uninstall-project` removes only receipt-owned integration files
and preserves any task authority. `papertiger-mise` remains a separate experimental campaign runner
and is never installed by planner setup.

## Wikitest evaluation

Source checkouts include `wikitest`, a separate workspace binary that exercises the public
`wikitool` executable rather than reaching through crate internals. It owns strict catalogs for
mechanical workflows, synthetic catalog retrieval, and externally authored and reviewed
prose assignments:

```bash
cargo build -p wikitool -p wikitest
target/debug/wikitest validate
target/debug/wikitest suite wikitool-capabilities --require-all
target/debug/wikitest prose prepare-suite prose-dogfood
target/debug/wikitest prose prepare-suite complex-prose-stress
# after every author and reviewer submission:
target/debug/wikitest prose evaluate-suite RUN
```

Wikitest campaigns are explicit development evaluations, not per-release gates. The deterministic
campaign covers 75 declared capability slices across 11 isolated scenarios; the complex prose
campaign prepares two source-rich long-form assignments and records no demonstrated coverage until
external authors and differently identified reviewers complete them. Agent submissions must record
their exact execution identity and access envelope. See `wikitest/CAPABILITY_MATRIX.md` for the
coverage and evidence limits.

Run-producing commands re-open and replay their hash-bound evidence—including the exact Wikitest
driver and evaluated Wikitool binaries—before returning success. Inspection labels the resulting
self-contained artifact set `unanchored`: an external immutable digest or signed build record is
still required for authenticity. A host wiki can add a read-only
supplemental catalog for its real local database without moving site doctrine into Wikitest or
Wikitool. Wikitest and its catalogs are opt-in development-evaluation substrate and are
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
| `agent-pack/integration/` | Generic agent and site-adapter contracts |
| `agent-pack/skills/` | Canonical editorial and operator skills |
| `wikitest/README.md` | Executable mechanical, catalog, and prose evaluation laboratory |
| `VERSIONING.md` / `CHANGELOG.md` | Release policy and history |

## License

AGPL-3.0-only, with supplementary terms in `LICENSE-SSL` and `LICENSE-VPL`.
