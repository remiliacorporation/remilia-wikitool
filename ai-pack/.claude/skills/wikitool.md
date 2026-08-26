# /wikitool - Thin wrapper

Use normal reasoning and verify current flags with `wikitool --help`, `wikitool <command> --help`,
and `docs/wikitool/reference.md`.

Read `writing_context/writing_guide.md`, `style_rules.md`, and `article_structure.md` before
authoring. You may research and write genuine encyclopedic prose. Build a claim-source map first;
model memory, search snippets, retrieval order, nearby pages, and fluent synthesis are not evidence.

At session start, inspect `wikitool status --modified --format json` and `wikitool diff --format json`,
then run `wikitool workflow session-refresh` and `wikitool knowledge status`. Do not use
`pull --overwrite-local` without explicit approval. Use `knowledge article-start
--intent new|expand|audit|refresh --view brief` for the typed coauthoring contract and
evidence signals. `drafting_ready` is artifact readiness, not publication readiness. Route new,
substantial, niche, or unclear article work through `/knowledge-interview`.

Use `knowledge contracts` for target template/module decisions and `validate --verify-live` for
production-sensitive links, redirects, and rendered-state findings.

Draft the factual body before the lead. Define the subject directly, keep claims within inspected
sources, and do not inherit Remilia, Charlotte Fang, Milady, category, or section framing from
adjacent pages. Run the adversarial reader edit before lint.

Use `article lint`, safe mechanical fixes, and scoped validation. A named human must read the exact
final prose and run or explicitly direct `article accept`; use the truthful origin, including
`agent-draft` or `collaborative-draft`. Never self-attest. Promotion and changed Main-namespace
pushes require the accepted hash, and `--force` cannot bypass it. Close with `review --view brief`,
`diff`, and `push --dry-run`.

Use normal web search to choose sources, then wikitool research/export for extraction and
provenance. Relay access challenges rather than bypassing them. After dynamic rendering changes,
use `wiki render-check` on each relevant consumer.
