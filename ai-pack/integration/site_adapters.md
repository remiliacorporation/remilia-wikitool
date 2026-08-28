# Site adapters

A site adapter is an explicit project-owned TOML file. Validate and select it during initialization:

```bash
wikitool init --adapter-path wikitool_adapter/site-adapter.toml
```

This records the following in `.wikitool/config.toml`:

```toml
[adapter]
path = "wikitool_adapter/site-adapter.toml"
```

The configured path must be project-relative, and its canonical target must remain under the
project root. Absolute paths, `..` escapes, and symlinks to external policy trees are rejected so
snapshots and isolated evaluations cannot depend on mutable files outside the project.

Without this section Wikitool uses the embedded `mediawiki-generic` adapter. It does not search executable ancestors or silently inherit a branded policy.

## Machine policy

The current strict document identity begins with:

```toml
schema_version = "site_adapter_v2"
adapter_id = "example-wiki"
docs_profile = "mw-1.44-authoring"
guidance_documents = []
```

`adapter_id` identifies the complete site policy. The contract does not imply adapter inheritance;
it is deliberately distinct from documentation-corpus profiles, authoring-surface profiles, and
projection target profiles.

The adapter can declare:

- short-description and article-quality mechanics;
- required appendices and reference template;
- citation template families and named-reference behavior;
- deterministic exact-host or subdomain rules that request source review;
- infobox preferences and category hints;
- mechanically decidable style constraints and placeholder artifacts;
- target extension and module availability contracts;
- relative supplemental guidance documents.

Unknown fields are rejected. Source-review hosts must be normalized lowercase hostnames; URL
substrings, schemes, paths, ports, and wildcards are not accepted as host rules. Guidance paths
must remain within the adapter directory. Wikitool hashes and exposes supplemental Markdown but
never parses it as executable policy.

Source-review rules are routing signals, not universal bans. Their reasons should tell the review skill what to inspect. Semantic exceptions stay in review findings rather than being hidden in substring logic.

Release packaging places the optional host bundle under `site_adapter/project/`, after validating
the policy and copying only its declared resources. Presence in a release archive does not activate
the adapter: the installed project must place it at a project-owned path and select that path in
`.wikitool/config.toml`.

## Supplemental guidance

Keep target names, relationships, local first-party-source rules, visual-subject conventions, and extension semantics in adapter Markdown. Generic skills read those documents after loading their public procedure. A host supplement may strengthen local requirements but should not redefine retrieval artifacts as evidence or claim that transactional publication acceptance authenticates an editor.

## Portability test

A standalone Wikitool checkout with no adapter should initialize offline, expose the generic adapter, and contain no target-wiki URL, relationship, template, category, or source verdict. A host project should regain all intended local behavior only after its explicit adapter is configured.
