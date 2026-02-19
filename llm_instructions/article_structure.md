# Article Structure Template

Every article follows this structure. Adapt sections to fit the subject â€” not every article needs every section.

---

## Required skeleton

```wikitext
{{SHORTDESC:Brief one-line description of the subject}}
{{Article quality|unverified}}

'''Article Title''' is [opening sentence defining the subject].

== Section heading ==
Content...

== References ==
{{Reflist}}

[[Category:Primary Category]]
[[Category:Secondary Category]]
```

## Line-by-line requirements

| Line | Content | Notes |
|---|---|---|
| 1 | `{{SHORTDESC:...}}` | Under 160 chars. Describes the subject, not the article. No wikitext inside. |
| 2 | `{{Article quality\|unverified}}` | Mandatory for AI-generated content. Never use `wip` or `verified`. |
| 3 | *(blank)* | |
| 4+ | `'''Bold Title'''` opening paragraph | First sentence defines the subject. Bold only the title, first occurrence. |

## Infobox placement

Place infoboxes after the quality banner, before the article body:

```wikitext
{{SHORTDESC:Generative NFT collection by Remilia Corporation}}
{{Article quality|unverified}}
{{Infobox NFT collection
|name        = Milady Maker
|image       = Milady_Maker_logo.png
|parent_group = Remilia
|supply      = 10000
|blockchain  = Ethereum
}}

'''Milady Maker''' is a generative NFT collection...
```

For Remilia projects: use `parent_group = Remilia` instead of `creator` or `artist`.

## Lead section

- 1-4 paragraphs summarizing the article.
- First sentence: `'''Subject'''` is/was [definition with context].
- No section heading â€” the lead comes before any `==` heading.
- Citations in the lead are optional if the same facts are cited in the body.

## Body sections

Adapt to your subject. Common patterns:

| Subject type | Typical sections |
|---|---|
| Person | History, Career, Notable work, Personal life |
| Organization | History, Projects, Structure, Impact |
| Concept | Origin, Description, Usage, Reception |
| NFT Collection | Development, Launch, Design, Community, Impact |
| Event | Background, The event, Aftermath |
| Artwork | Creation, Description, Exhibition, Reception |

### Section rules
- Use `==` for main sections, `===` for subsections.
- Sentence case: `== Early life ==` not `== Early Life ==`.
- One blank line before each heading.
- Do not number sections or phrase them as questions.

## Standard appendices (in this order)

```wikitext
== See also ==
* [[Related Article 1]]
* [[Related Article 2]]

== References ==
{{Reflist}}

== External links ==
* [https://example.com Official website]

[[Category:Remilia]]
[[Category:NFT Collections]]
```

### See also
- 3-5 links to related articles not already prominently linked in the body.
- Only link to pages that exist. Never add red links here.

### References
- `{{Reflist}}` renders all inline `<ref>` citations.
- This section is mandatory if the article has any citations.

### Further reading (optional)
- Relevant publications not used as sources. Be selective.

### External links (optional)
- Official websites, major resources. Typically 1-3 links maximum.
- For NFT collections, include `{{Etherscan}}` with the contract address.

### Categories
- 2-4 categories from the wiki's existing category set.
- Place at the very end of the article, after all sections.
- Look up valid categories: `wikitool search "Category:"`

## Content extension tags

Use these when the article calls for special content:

```wikitext
<math>E = mc^2</math>                                    <!-- Math formulas -->
<syntaxhighlight lang="solidity">code</syntaxhighlight>  <!-- Source code -->
<poem>Line 1\nLine 2</poem>                               <!-- Poetry/lyrics -->
{{#ev:youtube|VIDEO_ID|description=Caption}}              <!-- Video embeds -->
<tabber>
|-|Tab 1=Content
|-|Tab 2=Content
</tabber>                                                 <!-- Tabbed content -->
```

See `extensions.md` for full details on each.

