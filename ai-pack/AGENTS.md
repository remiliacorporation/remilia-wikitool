# Wikitool agent companion

Wikitool is a general MediaWiki substrate. The binary owns target configuration, capability discovery, local indexing, deterministic wikitext checks and fixes, source capture, revision-bound sync, atomic local writes, and the transactional publication-acceptance store. Agent skills own research judgment, prose, due weight, sensitive-claim review, and human interviewing. Project adapters own site-specific templates, categories, source-review signals, extensions, and editorial supplements.

Use `codex_skills/wikitool-operator/SKILL.md` for mechanical MediaWiki operations, `codex_skills/wiki-writing/SKILL.md` for authoring, `codex_skills/prose-review/SKILL.md` for independent editorial review, and `codex_skills/wiki-interview/SKILL.md` for human knowledge intake. Read the full selected skill and its routed references before acting.

Full release bundles also carry Papertiger as an optional, independently governed companion under `papertiger/`. Wikitool does not need it and must not initialize task authority implicitly. When a user chooses durable local planning, preview and apply Papertiger's own `setup-project`; then use the project-local Papertiger skill and canonical `tools/papertiger/agent_integration.md` contract installed by that command.

Run Wikitool from the active project root or pass `--project-root`. Use `wikitool <command> --help` as the authority for flags and `docs/wikitool/reference.md` for the generated command reference. Read `integration/agent_integration.md` for the boundary model and `integration/site_adapters.md` when configuring a target wiki.

Never treat model memory, retrieval rank, snippets, neighboring pages, or fluent synthesis as evidence. Never perform direct MediaWiki writes outside Wikitool. Use live read verification when local state may be stale. Do not claim that publication acceptance authenticates the named editor: the identity is explicitly self-reported and unauthenticated.
