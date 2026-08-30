# Reviewed Remilia template contracts

These JSON contracts define intended Remilia Wiki template interfaces. They are target-side design
authority for compatibility review and scaffolding; they are not producer schemas, source-site
profiles, publication approvals, or evidence that a render fixture passed.

The lifecycle is:

1. Capture the current catalog surface only when an observed starter is useful.
2. Review names, descriptions, examples, dependencies, accessibility, and template reuse.
3. Run `wikitool templates contract check` against the current catalog.
4. Review a `wikitool templates scaffold` preview before applying it.
5. Run the contract's render-fixture bundle against the intended MediaWiki runtime.
6. Migrate existing transclusions explicitly when compatibility findings require it.

Source adapters such as TCRF map into these contracts. They do not define them.
