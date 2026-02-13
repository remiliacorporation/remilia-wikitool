# Remilia Wiki — Writing Guide

You create encyclopedic articles for a MediaWiki wiki about Remilia Corporation, Milady Maker, and the network spirituality ecosystem.

**Wiki:** https://wiki.remilia.org
**Stack:** MediaWiki 1.44 with Lua templates, Cargo structured data, CirrusSearch

This guide is the primary reference. Read `style_rules.md` before every article.

---

## 1. Output format

All output must be raw MediaWiki wikitext, ready for direct use on the wiki. Never output Markdown. Never wrap output in code blocks. Never include commentary or meta-text — only article wikitext.

---

## 2. Article workflow

### Writing a new article

1. **Research the topic** — search the web for reliable sources. Check the live wiki for existing content and related articles to link.
2. **Read `style_rules.md`** — internalize the antipatterns before writing.
3. **Look up templates** — use `bun run wikitool context --template "Template Name"` to see parameters for any template. See section 6 for common infobox mappings.
4. **Write the article** following the structure in `article_structure.md`.
5. **Self-check** against the quick checklist at the end of `style_rules.md`.
6. **Save** to `wiki_content/Main/{Article_Title}.wiki`.

### Editing an existing article

1. Pull latest: `bun run wikitool pull`
2. Read the existing article.
3. Make changes following all the same rules.
4. Review: `bun run wikitool diff`
5. Push: `bun run wikitool push --dry-run -s "Summary"` then `bun run wikitool push -s "Summary"`

---

## 3. Research and sources

### This is a subcultural wiki, not an academic journal

This is the most important sourcing principle. Excessive academic citations are a telltale sign of AI writing. Prefer primary sources over academic papers.

**Good sources:**
- Official announcements, blog posts, project websites
- Tweets and social media posts (primary sources)
- News articles from established outlets
- Interviews and podcasts
- On-chain data (Etherscan, OpenSea)

**Avoid:**
- Academic journals (unless the claim is itself academic)
- Anonymous forum posts (unless notable in context)
- Unverified rumors
- Paywalled content you can't verify

**Never cite:**
- IQ.wiki — unreliable, user-generated
- Know Your Meme — tertiary source, quality issues
- NFT Price Floor — inaccurate details

### Verified wiki articles

Articles marked `{{Article quality|verified}}` represent editor-approved content. Use them to:
- Define project-specific terminology consistently
- Ensure consistent internal linking (`[[Remilia Corporation]]`, `[[Milady Maker]]`)
- Follow established formatting patterns

### When to search vs. when to write from knowledge

**Must search (cite the source):**
- Specific dates, events, names, actions
- Direct quotations
- Controversial or surprising claims
- Statistics, numbers, data points
- Reception and impact claims

**May write without searching (no citation needed):**
- Common knowledge: "NFTs are digital tokens recorded on blockchains"
- General background: "online communities often develop distinctive aesthetics"
- Technical context: "smart contracts execute automatically"
- Historical context: "the early internet fostered pseudonymous communities"

---

## 4. Citation strategy

### Target density

- **Short article (2-4 paragraphs):** 2-5 citations
- **Medium article (5-10 paragraphs):** 5-10 citations
- **Long article:** proportionally more, but never cite every sentence

Focus citations on the claims that matter most. Let general context breathe without citations.

### Citation templates

```wikitext
{{Cite web|url=|title=|author=|date=|access-date=2026-02-13|website=}}
{{Cite tweet|user=|number=|title=|date=}}
{{Cite news|url=|title=|author=|date=|access-date=2026-02-13|work=}}
{{Cite post|url=|title=|author=|date=|access-date=2026-02-13}}
{{Cite video|url=|title=|author=|date=|access-date=2026-02-13}}
```

- Fill in all available fields. Leave unknown fields empty (omit them).
- **Always leave archive fields blank** (`archive-url`, `archive-is`, `archive-date`, `screenshot`) — human editors complete these.
- Use `access-date` of today's date.

### Named references

When citing the same source multiple times:

```wikitext
First use: <ref name="fang2023">{{Cite web|...}}</ref>
Later:     <ref name="fang2023" />
```

Name conventions: `author+year` format, lowercase, no spaces. For multiple works by the same author in the same year: `fang2023a`, `fang2023b`.

Never duplicate full citations. Never declare named refs inside `{{Reflist}}`.

---

## 5. Content rules

### Remilia-specific

- **Attribution:** For Remilia projects, use `parent_group = Remilia` in infoboxes instead of `creator` or `artist` fields. Discuss individual contributors in the article body. This honors post-authorship principles.
- **Charlotte Fang:** Relevant but don't relate everything back to her. Use "Remilia" or "Remilia Corporation" as the subject unless specifically quoting her or discussing actions directly attributed to her.
- **Terminology:** Use terms as established in verified wiki articles (e.g., "network spirituality", not "digital spirituality").

### Quality marking

Every AI-generated article must include `{{Article quality|unverified}}` on line 2. Only human editors may change this to `wip` or `verified`. Never output `{{Article quality|wip}}` or `{{Article quality|verified}}`.

### Categories

Categories are managed via the wiki database. To find valid categories:

```bash
bun run wikitool search --categories          # List all categories
bun run wikitool search -c "Category:Name"    # Search specific category
```

General rules:
- Use 2-4 categories per article
- `[[Category:Remilia]]` goes on all Remilia-related content
- Choose the most specific applicable category
- Never invent categories — use only those that exist on the wiki

---

## 6. Infobox selection

| Subject type | Infobox |
|---|---|
| Person | `{{Infobox person}}` |
| Organization/Group | `{{Infobox organization}}` |
| NFT Collection | `{{Infobox NFT collection}}` |
| Artwork | `{{Infobox artwork}}` |
| Website/Platform | `{{Infobox website}}` |
| Concept/Philosophy | `{{Infobox concept}}` |
| Exhibition | `{{Infobox exhibition}}` |
| General/Other | `{{Infobox subject}}` |

To see all parameters for any template:

```bash
bun run wikitool context --template "Infobox person"
bun run wikitool context --template "Cite web"
```

This queries the live database, so it always reflects current template capabilities.

---

## 7. Looking up templates and extensions

### Template parameters

Use wikitool to query template parameters from the database:

```bash
bun run wikitool context --template "Template Name"   # Show params + usage stats
bun run wikitool search "infobox"                      # Find templates by name
```

This is always authoritative — it reflects what's actually deployed on the wiki.

### Extension documentation

Extension docs are imported from mediawiki.org and searched locally:

```bash
bun run wikitool docs search "embed video"             # Search imported docs
bun run wikitool docs list                              # List all imported docs
bun run wikitool docs import ExtensionName              # Import new extension docs
bun run wikitool docs update                            # Refresh all imported docs
```

See `extensions.md` for a quick reference of the most-used content tags.

### Categories

```bash
bun run wikitool search "Category:"                    # Browse categories
```

---

## 8. Reference files

| File | Purpose | When to read |
|---|---|---|
| `style_rules.md` | Natural writing antipatterns | **Before every article** |
| `article_structure.md` | Structural template | Before writing new articles |
| `extensions.md` | Quick reference for content extension tags | When using math, code, video, tabs |

For template parameters and categories, always use wikitool live lookups rather than static files.
