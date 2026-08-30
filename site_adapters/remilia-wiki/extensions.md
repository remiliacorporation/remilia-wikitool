# Remilia Wiki content mechanisms

The machine-readable availability contract is in `site-adapter.toml`. Confirm the live capability manifest before using an extension, because deployment can drift from the repository adapter.

## Common mechanisms

- Math: `<math>E = mc^2</math>`
- Syntax highlighting: `<syntaxhighlight lang="solidity">…</syntaxhighlight>`
- Poetry: `<poem>…</poem>`
- Video: `{{#ev:youtube|VIDEO_ID}}`
- Tabs: `<tabber>…</tabber>`
- Galleries: `<gallery>…</gallery>`
- Dynamic page lists: `<DPL>…</DPL>`
- Category trees: `<categorytree>Category name</categorytree>`
- Language markup: `{{lang|fr|deja vu}}`

Use a rich mechanism only when it improves comprehension. Validate its syntax and inspect rendered output.

## D3Chart local contract

Remilia Wiki currently renders D3 charts through `Module:D3Chart` and the `ext.d3charts.loader` ResourceLoader module. This is a Remilia-local mechanism, not generic MediaWiki syntax.

```wikitext
{{#invoke:D3Chart|bar
|data=Milady:100,Remilio:50,Bonkler:10
|title=Example distribution
|xLabel=Collection
|yLabel=Count
|showFrame=true
|gridStyle=dotted
}}
```

Current module chart types are `bar`, `hbar`, `line`, `pie`, `donut`, `scatter`, and `area`. Manual non-scatter data uses `label:value` pairs; scatter data uses `x:y` or `label:x:y` pairs. Cargo-backed charts use `table=` or `tables=` with fields such as `label=`, `value=`, `x=`, and `y=`.

Do not add raw `<script>` tags, inline D3 JavaScript, or hand-written `.d3-chart` containers to article wikitext. If the live module contract differs, stop and reconcile the adapter rather than guessing.
