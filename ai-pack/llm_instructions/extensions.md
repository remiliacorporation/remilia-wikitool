# Content Extension Tags

Quick reference for MediaWiki extension tags used in articles.
Only content-formatting extensions relevant to article writing are listed here.

For detailed or up-to-date extension documentation, use wikitool:

```bash
wikitool docs search "extension name"
wikitool docs update
```

---

## Math (Native MathML)

Render mathematical formulas using LaTeX syntax.

```wikitext
<math>E = mc^2</math>
<math>\sum_{i=1}^{n} i = \frac{n(n+1)}{2}</math>
```

## Code highlighting (SyntaxHighlight)

```wikitext
<syntaxhighlight lang="solidity">
contract Milady {
    uint256 public totalSupply = 10000;
}
</syntaxhighlight>
```

## Poetry and lyrics (Poem)

```wikitext
<poem>
This is the first line.
This is the second line.
    Indented lines work too.
</poem>
```

## Video embeds (EmbedVideo)

```wikitext
{{#ev:youtube|dQw4w9WgXcQ}}
{{#ev:youtube|dQw4w9WgXcQ|description=Video caption here}}
{{#ev:vimeo|12345678}}
```

## Tabbed content (TabberNeue)

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

## Dynamic page lists (DPL4)

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

```wikitext
<categorytree>NFT Collections</categorytree>
```

## Non-English text

```wikitext
{{lang|fr|deja vu}}
{{lang|ja|kawaii}}
```
