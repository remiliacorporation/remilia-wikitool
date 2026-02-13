# Content Extension Tags

Quick reference for MediaWiki extension tags used in articles.
Only the content-formatting extensions relevant to article writing are listed here.

For detailed or up-to-date extension documentation, use wikitool:

```bash
bun run wikitool docs search "extension name"    # Search imported docs
bun run wikitool docs update                      # Refresh from mediawiki.org
```

---

## Math (Native MathML)

Render mathematical formulas using LaTeX syntax.

```wikitext
<math>E = mc^2</math>
<math>\sum_{i=1}^{n} i = \frac{n(n+1)}{2}</math>
```

Uses Native MathML rendering (supported in modern browsers).

## Code highlighting (SyntaxHighlight)

Color-formatted source code blocks.

```wikitext
<syntaxhighlight lang="solidity">
contract Milady {
    uint256 public totalSupply = 10000;
}
</syntaxhighlight>
```

Supported languages include: python, javascript, typescript, php, css, html, sql, lua, solidity, rust, go, and many more.

## Poetry and lyrics (Poem)

Preserves line breaks automatically.

```wikitext
<poem>
This is the first line.
This is the second line.
    Indented lines work too.
</poem>
```

## Video embeds (EmbedVideo)

Embed external video from YouTube, Vimeo, Twitch, etc.

```wikitext
{{#ev:youtube|dQw4w9WgXcQ}}
{{#ev:youtube|dQw4w9WgXcQ|description=Video caption here}}
{{#ev:youtube|dQw4w9WgXcQ|640x360}}
{{#ev:vimeo|12345678}}
{{#ev:twitch|channelname}}
```

YouTube defaults to 640x360. Audio embeds are also supported (SoundCloud, Spotify).

## Tabbed content (TabberNeue)

Create tabbed interfaces within articles.

```wikitext
<tabber>
|-|Background=
[[File:Example_bg.png|thumb|Blue background]]
Blue backgrounds appear on 15% of items.
|-|Hair=
[[File:Example_hair.png|thumb|Hair variants]]
Over 50 unique hair styles exist.
</tabber>
```

Use for: trait categories, version comparisons, organizing lengthy content.

## Uploaded media (TimedMediaHandler)

Embed uploaded video and audio files.

```wikitext
[[File:Video_name.webm|thumb|right|300px|Caption here]]
```

Supports WebM, Ogg, and MP4. Automatically transcodes to 480p and 720p.

## Dynamic page lists (DPL4)

Generate lists of pages based on categories or other criteria. Use sparingly — these are resource-intensive.

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

Display category contents as a tree.

```wikitext
<categorytree>NFT Collections</categorytree>
```

Primarily for category pages and portals, not article prose.

## Non-English text

Mark non-English words for accessibility and semantic markup:

```wikitext
{{lang|fr|déjà vu}}
{{lang|ja|カワイイ}}
```

Required for all non-English text in articles. Proper names typically don't need this.
