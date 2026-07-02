# Content Extension Tags

Quick reference for MediaWiki extension tags used in articles.
Only content-formatting extensions relevant to article writing are listed here.

Each section carries a machine-readable `Contract:` line (`key=value` pairs separated by `;`)
that wikitool parses into the profile overlay, so lint, the authoring surface, and this document
share one contract per mechanism. Keep the line in sync with the prose when editing.

For detailed or up-to-date extension documentation, use wikitool:

```bash
wikitool docs search "extension name"
wikitool docs update
```

---

## Math (Native MathML)

Contract: kind=tag; name=math; provider=Math; syntax=paired; body=required

Render mathematical formulas using LaTeX syntax.

```wikitext
<math>E = mc^2</math>
<math>\sum_{i=1}^{n} i = \frac{n(n+1)}{2}</math>
```

## Code highlighting (SyntaxHighlight)

Contract: kind=tag; name=syntaxhighlight; provider=SyntaxHighlight; syntax=paired; body=required; attributes=lang

```wikitext
<syntaxhighlight lang="solidity">
contract Milady {
    uint256 public totalSupply = 10000;
}
</syntaxhighlight>
```

## Poetry and lyrics (Poem)

Contract: kind=tag; name=poem; provider=Poem; syntax=paired; body=required

```wikitext
<poem>
This is the first line.
This is the second line.
    Indented lines work too.
</poem>
```

## Video embeds (EmbedVideo)

Contract: kind=parser_function; name=ev; provider=EmbedVideo; syntax=parser_function

```wikitext
{{#ev:youtube|dQw4w9WgXcQ}}
{{#ev:youtube|dQw4w9WgXcQ|description=Video caption here}}
{{#ev:vimeo|12345678}}
```

## Tabbed content (TabberNeue)

Contract: kind=tag; name=tabber; provider=TabberNeue; syntax=paired; body=required

```wikitext
<tabber>
|-|Background=
[[File:Example_bg.png|thumb|Blue background]]
|-|Hair=
[[File:Example_hair.png|thumb|Hair variants]]
</tabber>
```

## Uploaded media (TimedMediaHandler)

```wikitext
[[File:Video_name.webm|thumb|right|300px|Caption here]]
```

## Image galleries

Contract: kind=tag; name=gallery; provider=core; syntax=paired; body=required; attributes=mode,widths,heights,perrow

```wikitext
<gallery>
File:Example1.png|Caption one
File:Example2.png|Caption two
File:Example3.png|Caption three
</gallery>
```

Optional attributes: `mode="packed"`, `widths=200px`, `heights=150px`, `perrow=4`.

## Dynamic page lists (DPL4)

Contract: kind=tag; name=dpl; provider=DPL4; syntax=paired; body=required

```wikitext
<DPL>
category = NFT Collections
count = 10
ordermethod = lastedit
order = descending
format = ,* [[%PAGE%]]\n,,
</DPL>
```

## Category trees

Contract: kind=tag; name=categorytree; provider=CategoryTree; syntax=paired; body=required

```wikitext
<categorytree>NFT Collections</categorytree>
```

## Non-English text

Contract: kind=template; name=lang; provider=core; syntax=template

```wikitext
{{lang|fr|deja vu}}
{{lang|ja|kawaii}}
```

## D3Charts (Remilia local contract)

Contract: kind=module; name=D3Chart; provider=local; syntax=module_invoke

Remilia Wiki currently renders D3 charts through `Module:D3Chart` plus the `ext.d3charts.loader`
ResourceLoader module. Use this only when the target wiki's local profile/template surfaces expose
`Module:D3Chart`; it is not generic MediaWiki syntax and may be replaced by a bespoke extension.

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

Supported current module chart types are `bar`, `hbar`, `line`, `pie`, `donut`, `scatter`, and
`area`. Manual non-scatter data uses `label:value` pairs; scatter data uses `x:y` or `label:x:y`
pairs. Cargo-backed charts use `table=`/`tables=` plus fields such as `label=`, `value=`, `x=`,
and `y=`.

Do not add raw `<script>` tags, inline D3 JavaScript, or hand-written `.d3-chart` containers to
article wikitext. Use the local module/extension contract and run `wikitool article lint`; if the
wiki later ships a dedicated D3 extension, follow the live target profile and extension docs instead
of this module form.
