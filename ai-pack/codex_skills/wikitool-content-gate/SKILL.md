---
name: wikitool-content-gate
description: Critically review and gate human-, agent-, or collaboratively drafted MediaWiki prose with source-fidelity checks, deterministic lint, exact human acceptance, validation, dry-run review, and guarded push.
---

# Wikitool content gate

Use editorial judgment and verify commands with `wikitool --help`, `wikitool <command> --help`, and
`docs/wikitool/reference.md`. Read `writing_context/style_rules.md` before judging prose.

Agent authorship is allowed. Model output is not evidence. Mechanical success is not the standard: a clean lint report cannot
show that citations support their claims, the article defines its subject, coverage is
proportionate, or someone would want to read it.

For a draft:

1. Read the article as a reader and apply the hard failures and final reader test in
   `style_rules.md`.
2. Inspect the sources behind sensitive, interpretive, quoted, surprising, and load-bearing claims.
3. Run `wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json
   --view brief --summary "Draft review"`.
4. Follow `next_steps` for direct `article lint` and safe mechanical `article fix` operations.
5. Have a named human read the exact prose. Only that human may run or explicitly direct `article accept
   ... --human-editor ID --prose-origin ORIGIN`; the agent must not self-attest.
6. Run `article promote`. Promotion fails if the file differs from the accepted hash.
7. Run scoped `wikitool review --format json --view brief --summary "Editorial review"`, `diff`,
   and `push --dry-run` after promotion.

Use `agent-draft` or `collaborative-draft` when truthful; provenance is useful and is not itself a
quality verdict. The human acceptance receipt attests that the exact article was judged specific,
readable, proportionate, and source-bound. The editor identity is an audit assertion, not
cryptographic authentication. Any content change invalidates the receipt. `--force` cannot bypass
it.

Use `knowledge inspect references duplicates`, `validate --summary`, and targeted `validate
--category ... --verify-live` as applicable. After a live template, Cargo, or dynamic HTML change,
run `wiki render-check` for every consumer shape whose rendered behavior matters.
