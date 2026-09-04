# Template migrations and render evidence

Use contracts to describe the intended template surface, render fixtures to check the target,
and migration plans to inspect the current source bytes. None of these observations authorizes
publication or proves that a template can be removed from a live wiki.

## Render assertions

Every `render_fixtures` entry may include `dom_assertions` and
`forbid_nested_interactive`. For example:

```json
{
  "id": "notice",
  "invocation": "{{Notice|text=Read the source}}",
  "scope_class": "notice",
  "expected_scope_count": 1,
  "forbid_nested_interactive": true,
  "dom_assertions": [
    {"selector": "[role=note]", "max_count": 1, "attributes": {"aria-label": "Source notice"}},
    {"selector": "h2", "max_count": 1, "text_contains": "Source"},
    {"selector": "img", "min_count": 0, "attributes": {"alt": null}},
    {"selector": "button:not([aria-label])", "min_count": 0, "max_count": 0}
  ]
}
```

Each assertion runs independently in every matching scope, including its root. Without
`scope_class`, assertions apply to the complete parsed fragment. `min_count` defaults to one;
`max_count` is optional. Every selected element must satisfy every requested attribute and text
condition. An attribute value of `null` requires presence; a string requires the exact decoded
value. Text comparison normalizes whitespace but does not invent separators between text nodes.
Missing scopes fail even when a negative assertion permits zero elements. The optional nesting
check inspects server markup before HTML tree repair, catching nested links that a browser would
reparent. It detects interactive elements inside links and buttons, not every HTML conformance rule.

Contracts admit at most 64 assertions, 256 bytes per selector, 32 attributes per assertion, and
4096 bytes per text or attribute value. Invalid selectors or contradictory count limits fail before
the network request. Reports use `render_check_v3` and include individual match/mismatch counts.
More than 100000 DOM elements or 4096 scope/assertion combinations produces an explicit unevaluated
failure. A zero count on such a result is not an observation about the page.

These are assertions about server HTML, including explicit names, roles, headings, and alt
attributes. They do not compute the accessible-name algorithm, visibility, CSS cascade, focus
behavior, contrast, or viewport geometry. Every report declares `browser_layout: "not_measured"`.
Use browser testing at the required viewports before accepting responsive or accessibility claims.

## Exact migration plans

Create a project-owned specification:

```json
{
  "schema": "template_migration_spec_v1",
  "from_template": "Template:Old notice",
  "to_template": "Template:Notice",
  "title_case": "first_letter",
  "parameter_renames": {"message": "text"},
  "deprecated_parameters": ["legacy_style"]
}
```

Set `title_case` from the target's siteinfo `case` value: `first_letter` or `case_sensitive`.
Namespace and underscore normalization do not make the rest of a title case-insensitive.
Parameter names preserve case, underscores, and internal whitespace.

```bash
wikitool templates migration-plan .wikitool/notice-migration.json --format json
```

The planner reads all locally discovered `.wiki` files directly. It does not depend on a fresh
catalog, resolve redirects, inspect remote usage, rewrite files, or execute a migration. Its plan
identity binds the specification and the full scanned file/hash inventory, including zero-match
files. Each affected page includes a full source SHA-256, exact invocation byte spans and hashes,
small nonoverlapping key/title patches, and the resulting candidate hash when unambiguous.
Whitespace, values, Unicode, comments, literal regions, and nested invocations remain intact.

Duplicate or newly colliding parameters, implicit positional renames, deprecated values,
dynamic names, and unfinished braces require review. Such a file exposes no actionable patches
or candidate hash. Dynamic constructs in other files remain visible as limits on the inventory.
The scan refuses more than 20000 files, 4 MiB per source, or 256 MiB of wikitext.

Inspect every affected invocation. Recheck original hashes before editing; compare before/after
render fixtures against the intended target; verify live transclusions and redirect chains; then
rescan after the migration. `retirement_ready` remains false because local absence alone cannot
prove that a live compatibility shim is unused. Keep redirects or shims until that independent
verification succeeds. Use ordinary revision-bound Wikitool review and push for publication.
