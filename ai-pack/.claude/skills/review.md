# /review - Thin wrapper

Use normal reasoning and editorial judgment. Verify current commands with `wikitool --help`,
`wikitool <command> --help`, and
`docs/wikitool/reference.md`. Read `writing_context/style_rules.md` before reviewing prose.

Agent authorship is allowed. A clean mechanical report does not prove source fidelity,
readability, proportion, or reader value. Read the article first, inspect the evidence behind
load-bearing and sensitive claims, and apply the hard failures and final reader test.

For a draft, run `wikitool review --draft-path ... --view brief`, then follow `next_steps` for direct lint and
safe fixes. Have a named human read the exact prose and run or explicitly direct `article accept`
with the truthful origin; the agent must not self-attest. Run `article promote`, then scoped
`wikitool review --format json --view brief --summary "Editorial review"`, `diff`, and
`push --dry-run`. The hash-bound receipt is an audit assertion, not
cryptographic authentication; any prose change invalidates it and `--force` cannot bypass it.

Use `knowledge inspect references duplicates`, `validate --summary`, targeted `validate
--verify-live`, and rendered-consumer checks where applicable.
