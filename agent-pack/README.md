# Wikitool agent pack

This directory is Wikitool's canonical, harness-neutral agent companion. It contains four complete
[Agent Skills](https://agentskills.io/) packages under `skills/` and architecture notes under
`integration/`.

The built pack is shipped as `agent/` beside the Wikitool binary. Install it into a project with:

```text
wikitool agent inspect
wikitool agent setup-project /path/to/project --target auto
```

`auto` installs into `.agents/skills/`, `.claude/skills/`, or both according to existing project
markers. An unmarked project defaults to `.agents/skills/`. Use `--target agents`, `--target
claude`, or `--target both` to make the destination explicit.

Setup copies identical canonical packages; it does not generate wrappers or edit `AGENTS.md` or
`CLAUDE.md`. Wikitool records exact file identities in `.wikitool-agent/project-install.json`.
Re-running setup upgrades or repairs only unchanged receipt-owned files. Modified or foreign files
inside a Wikitool-owned skill directory stop the operation. The same checks protect
`uninstall-project` from deleting user work.

## Product boundary

| Layer | Owns |
|---|---|
| Wikitool binary | MediaWiki transport and discovery, parsing, catalogs, deterministic lint/fix, revision CAS, durable mutation receipts, and exact-content acceptance records |
| Agent skills | research-to-claim reasoning, article prose, source fidelity, due weight, sensitive-claim review, reader value, and adaptive interviews |
| Project site adapter | target templates, citation forms, extension capabilities, mechanical policy, and supplemental local editorial guidance |
| Human editor | publication intent, private context, disputed judgments, and acceptance of the exact final prose |

The agent pack is intentionally separate from release support data. Release archives also contain
documentation and the generic and Remilia Wiki adapter templates, but those are not agent skills
and are not installed by `wikitool agent setup-project`.

## Included skills

- `wikitool-operator`: mechanical Wikitool retrieval, validation, sync, and receipt operations.
- `wiki-interview`: structured human knowledge intake before research or drafting.
- `wiki-writing`: evidence-bound encyclopedic authoring and substantial revision.
- `prose-review`: independent source-fidelity, due-weight, BLP, and reader-value review.

Add a top-level skill only when it has a distinct trigger, procedure, and output authority. Put
scenario-specific rigor in a required reference or conditional branch of an existing skill.

See `integration/agent_integration.md` and `integration/site_adapters.md` for the architectural
contract.
