# Testing Wikitool

Use one authority for each kind of evidence:

| Question | Test authority |
|---|---|
| Does a parser, plan, state transition, or failure boundary obey its contract? | Focused Rust tests beside the implementation |
| Does the public executable perform a useful workflow correctly? | Wikitest scenario with exact exit codes, JSON/file assertions, and observed HTTP requests |
| Does a packaged artifact install and run on its target platform? | Native release jobs using the actual packaged binary and companions |
| Does a real wiki/corpus behave correctly? | Explicit read-only host scenario or target-specific operational check |
| Is the prose faithful, proportionate, and useful? | External author/reviewer campaign; preparation and mechanical success grant no editorial approval |

## Routine regression suite

```text
cargo test --workspace
cargo test --workspace --all-features
cargo build -p wikitool -p wikitest
target/debug/wikitest validate
target/debug/wikitest suite wikitool-regressions --require-all
```

CI runs the regression suite on Linux and Windows. It requires every declared capability, rejects
skips, and retains receipts with the exact evaluated binaries. Rebuild the default end-user binary
after an all-features test or maintainer build before evaluating it. The full
`wikitool-capabilities` suite additionally exercises conversion, companion inspection, and broader
MediaWiki reading. Choose it for changes to those surfaces or a comprehensive review.

A failed suite can have valid, replayable evidence. `inspect` reports the original failed status
and verifies only the capabilities actually demonstrated; it rejects a forged pass or invented
coverage. Run output identifies the failed scenario and its receipt, while individual runs print
the failed assertion details.

`validate` checks strict manifests, suite membership, coverage references, and every mechanical
scenario's copied input and MediaWiki fixture. It checks their exact bytes before execution; it
does not execute the tested tool or certify prose source packets. Execution checks inputs again.
Keep fixtures in LF form so their committed hashes survive a fresh Windows or Unix checkout.

## Writing a useful regression

Choose the smallest existing scenario that owns the behavior. Add a new scenario when the initial
state or failure mode needs independent isolation, not for every command spelling. Bind capability
labels to the steps that actually establish the claim. A capability count is a declaration of
tested slices, not line coverage or a quality score.

Check a consequential result: a specific JSON field, exact changed bytes, an unmodified file, a
revision-bound mutation, or a counted HTTP request. Avoid generic output words such as `error`,
`result`, or `import`. Prefer structured assertions for JSON; human-readable output assertions are
appropriate where a command has no structured format. Never discard a failing exit status.

Pair successful behavior with a relevant rejection or boundary when it protects distinct behavior.
For example, the docs-discovery family checks extension types and successful import, then proves
that API errors and absent discovery evidence stop before another request. A zero-request assertion
must inspect the fixture's complete observation, not infer absence from the command's error text.

Keep algorithm edge cases and evaluator defenses in Rust: process-tree termination, truncated
output, path isolation, strict decoding, receipt tampering, stale identities, and independent prose
review bindings are distinct invariants. Do not replace them with a broad happy-path scenario, or
duplicate the same CLI workflow behind another temporary server.

## Retired harness coverage

The former `tests/cli_integration/` Bash harness, its Python server, and two docs CLI test files are
removed. Their useful checks now have the following owners; this table is a migration map, not a
claim that generic output matching established equivalent behavior.

| Former checks | Current owner |
|---|---|
| Init, explicit adapter, lint/fix, exact acceptance | `mechanical-article-lifecycle` |
| Catalog readiness, stats, chunks, backlinks | `catalog-indexed-retrieval` |
| Orphans, empty categories, template usage/implementation | `catalog-structural-audit` |
| Draft review, interview ledger, post-promotion validation | `mechanical-agent-authoring-workflow` |
| Status/diff, pull, non-mutating delete/push previews | `mechanical-mediawiki-sync` with exact plans and request observations |
| Template dependency closure | `mechanical-template-closure` |
| Contract/scaffold/capture and overwrite refusal | `mechanical-template-contracts` |
| Docs import/list/search/context/symbols/removal and substring retrieval after reset | `mechanical-knowledge-workspace` |
| Docs default profile and explicit override | `mechanical-knowledge-workspace`, replacing `docs_profile_defaults.rs` |
| Installed extension discovery and API failure | `mechanical-docs-discovery*`, replacing `docs_installed_discovery.rs` |
| Database stats/reset, Cargo CSV/JSON imports, LSP configuration, module lint | `mechanical-knowledge-workspace` |
| Retired command rejection | Existing Clap parser tests in `crates/wikitool/src/main.rs` |
| Deterministic skill build, ownership-safe install/uninstall, release payload layout | Existing maintainer/skills Rust tests and native release artifact jobs |
| Real Contextmink/Papertiger setup, upgrade and uninstall | Native release artifact jobs with actual pinned companion packages |
| Git worktree hook destination | Maintainer test in `crates/wikitool/src/dev_cli.rs`, including exact bytes and Unix executable mode |
| Optional live output-word checks | Retired as unreliable evidence; explicit host/operational checks own claims about real services |

No browser-layout or external prose-quality coverage is implied by these mechanical checks.
Historical receipts retain their original schemas and driver identities; do not rewrite them to
make a new binary replay old evidence.
