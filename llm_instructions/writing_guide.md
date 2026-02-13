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
7. **Validate** — run `bun run wikitool validate "Article Title"` to catch structural issues.

### Editing an existing article

1. Pull latest: `bun run wikitool pull`
2. Read the existing article.
3. Make changes following all the same rules.
4. Validate: `bun run wikitool validate "Article Title"`
5. Review: `bun run wikitool diff`
6. Push: `bun run wikitool push --dry-run -s "Summary"` then `bun run wikitool push -s "Summary"`

### Article length

Let content dictate length — don't pad thin topics or compress rich ones.

- **Stub** (1-2 paragraphs): acceptable for minor topics with limited sources
- **Short** (3-5 paragraphs + infobox): most articles
- **Medium** (8-15 paragraphs): major topics like Milady Maker, Remilia Corporation
- **Long** (15+ paragraphs): rare, reserved for flagship articles with deep sourcing

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
- Urban Dictionary — unmoderated, unverifiable

### Tone calibration

This is a subcultural wiki, not an academic journal. The tone should be encyclopedic but not dry or clinical. Match the register of good Wikipedia articles about internet culture — factual, clear, and willing to engage with cultural context without editorializing. Humor and irreverence are fine when sourced; promotional enthusiasm and clinical detachment are both wrong.

### Never fabricate

Never fabricate facts, dates, quotes, or source URLs. If a specific detail cannot be found, omit it rather than guessing. Mark uncertain claims with attribution: "According to [source]..." rather than asserting directly. Every URL and date in a citation must be real and verifiable.

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

### Internal linking

- Link to existing wiki articles on first mention in the body: `[[Remilia Corporation]]`, `[[Milady Maker]]`
- Link each article once — first occurrence only, don't re-link in later paragraphs
- Check if target exists: `bun run wikitool search "Article Name"`
- Never place red links in See also sections
- Use piped links when display text differs: `[[Remilia Corporation|Remilia]]`

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
