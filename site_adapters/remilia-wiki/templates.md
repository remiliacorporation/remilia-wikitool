# Remilia Wiki template vocabulary

Remilia Wiki's templates are the target vocabulary for imported and preserved content. Source
profiles describe how to recognize source markup and, when wikitext is available, map its template
semantics; they never grant the source site's templates or presentation conventions authority over
Remilia output. Conversion should express the source meaning through the smallest suitable Remilia
primitive, preserve ordinary wikitext when no template adds value, and surface an unmapped structure
when the target vocabulary cannot represent something without loss.

Do not create one Remilia template for every source-site template or visual variation. Prefer a
small stable interface with descriptive parameter names, TemplateData, accessible examples, and
explicit render fixtures. Extend an existing contract when the new case has the same meaning and
constraints. Create a specialized template only when the semantics or validation rules genuinely
differ. `Template:Infobox subject` is a last-resort fallback; use a subject-specific infobox when
one fits.

Article-level source maintenance boxes map to `Template:Ambox` only after the source profile admits
their observed table class and shape; section-scoped source notices use `Template:Section notice`.
Use `image_content` for a source-neutral preservation image; `image` is reserved for a native wiki
file name. Preserve the source message text and leave its visual subtype at the target default
unless captured semantics justify a more specific value.

Reviewed contracts live in `template_contracts/`. They are Remilia-owned target designs, not
captured truth. `wikitool templates contract capture` may produce a useful observed starter, but
that output must be reviewed and completed before it becomes a contract. Check compatibility and
dependency parity before scaffolding, inspect the preview plan, and execute the emitted render
fixtures against a representative MediaWiki runtime before accepting a template change. Parameter
renames require explicit migration mappings; Wikitool does not rewrite transclusions implicitly.

Preservation templates accept content-addressed archive identities and remain source-neutral.
`Template:Preservation image` and `Template:Preservation audio` can render objects from any
published PreservationArchive site generation. They must not embed TCRF-specific labels, routes,
or template assumptions.

Repeated source semantics should use the native target vocabulary when the mapping is exact.
`Template:Section notice` represents section-scoped source messages without an asymmetric accent
bar. `Template:Version label` and `Template:Version range` retain normalized software-version
components without importing a source wiki's release numbering. `Template:Claim status` preserves
source-authored verification state and its stated source, and `Template:Table cell status` emits a
structural yes/no/unknown table cell while distinguishing an omitted label from an explicitly blank
one. These interfaces are source-neutral; source profiles remain responsible for interpreting
source-specific parameter names and for rejecting or visibly preserving any semantic gap.
