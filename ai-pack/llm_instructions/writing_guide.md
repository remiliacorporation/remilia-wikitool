# Remilia Wiki â€” Writing Guide

You create encyclopedic articles for a MediaWiki wiki about Remilia Corporation, Milady Maker, and the network spirituality ecosystem.

**Wiki:** https://wiki.remilia.org
**Stack:** MediaWiki 1.44 with Lua templates, Cargo structured data, CirrusSearch

This guide is the primary reference. Read `style_rules.md` before every article.

---

## 1. Output format

All output must be raw MediaWiki wikitext, ready for direct use on the wiki. Never output Markdown. Never wrap output in code blocks. Never include commentary or meta-text â€” only article wikitext.

---

## 2. Article workflow

### Writing a new article

1. **Read `style_rules.md`** â€” internalize the antipatterns before writing.
2. **Refresh local authoring state** â€” run `wikitool knowledge warm --docs-profile remilia-mw-1.44` and `wikitool wiki profile sync` so docs/profile/capability signals are current.
3. **Build the interpreted authoring brief** â€” run `wikitool knowledge article-start “<Topic>” --intent new --format json`. This is the front door. The `section_skeleton` shows which sections comparable pages use; `content_backed` flags tell you which sections already have evidence in the pack. For sections where `content_backed` is `false`, use `wikitool knowledge inspect chunks` to fetch targeted content before writing.
4. **Fetch external evidence selectively** â€” use `wikitool research search “<Topic>” --format json`, then `wikitool research fetch “<URL>” --output json` only for sources you expect to cite. If fetch output has `status: "error"`, treat it as a source-access failure; do not cite challenge pages, blocked fetches, or fetch diagnostics as article evidence.
5. **Look up templates and profile rules** â€” use `wikitool templates show "Template:Template Name"`, `wikitool templates examples "Template:Template Name" --limit 2`, and `wikitool wiki profile show --format json`.
6. **Write the article** following the structure in `article_structure.md`.
7. **Save** to `wiki_content/Main/{Article_Title}.wiki`.
8. **Run article-aware lint** â€” `wikitool article lint wiki_content/Main/{Article_Title}.wiki --format json`. If the fixes are purely mechanical, follow with `wikitool article fix wiki_content/Main/{Article_Title}.wiki --apply safe`. For large reference cleanups, use `wikitool knowledge inspect references summary --title "{Article_Title}" --format json` and `wikitool knowledge inspect references duplicates --title "{Article_Title}" --format json`.
9. **Review** â€” run `wikitool review --format json --summary "Summary"` before push. Use `wikitool validate --summary` for the lower-level global integrity signal and scoped validation flags when investigating a specific issue.

Use `wikitool knowledge pack ... --format json` when you need the deeper raw retrieval bundle behind `article-start`.

### Editing an existing article

1. Pull latest: `wikitool pull`
2. Read the existing article.
3. Make changes following all the same rules.
4. Lint the draft: `wikitool article lint wiki_content/Main/{Article_Title}.wiki --format json`
5. Review: `wikitool review --title "{Article_Title}" --format json --summary "Summary"`
6. Diff: `wikitool diff --title "{Article_Title}"`
7. Preflight conflicts: `wikitool push --dry-run --title "{Article_Title}" --summary "Summary"`
8. Push: `wikitool push --title "{Article_Title}" --summary "Summary"`

### Article length

Let content dictate length â€” don't pad thin topics or compress rich ones.

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
- IQ.wiki â€” unreliable, user-generated
- Know Your Meme â€” tertiary source, quality issues
- NFT Price Floor â€” inaccurate details
- Urban Dictionary â€” unmoderated, unverifiable

### Tone calibration

This is a subcultural wiki, not an academic journal. The tone should be encyclopedic but not dry or clinical. Match the register of good Wikipedia articles about internet culture â€” factual, clear, and willing to engage with cultural context without editorializing. Humor and irreverence are fine when sourced; promotional enthusiasm and clinical detachment are both wrong.

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
{{Cite web|url=|title=|author=|date=|access-date=YYYY-MM-DD|website=}}
{{Cite tweet|user=|number=|title=|date=}}
{{Cite news|url=|title=|author=|date=|access-date=YYYY-MM-DD|work=}}
{{Cite post|url=|title=|author=|date=|access-date=YYYY-MM-DD}}
{{Cite video|url=|title=|author=|date=|access-date=YYYY-MM-DD}}
```

- Fill in all available fields. Leave unknown fields empty (omit them).
- **Always leave archive fields blank** (`archive-url`, `archive-is`, `archive-date`, `screenshot`) â€” human editors complete these.
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
- Link each article once â€” first occurrence only, don't re-link in later paragraphs
- Check if target exists: `wikitool search "Article Name"`
- Never place red links in See also sections
- Use piped links when display text differs: `[[Remilia Corporation|Remilia]]`

### Quality marking

Every AI-generated article must include `{{Article quality|unverified}}` on line 2. Only human editors may change this to `wip` or `verified`. Never output `{{Article quality|wip}}` or `{{Article quality|verified}}`.

### Categories

Categories are managed via the wiki database. To find valid categories:

```bash
wikitool search "Category:"            # List/browse categories
wikitool search "Category:Name"    # Search specific category
```

General rules:
- Use 2-4 categories per article
- `[[Category:Remilia]]` goes on all Remilia-related content
- Choose the most specific applicable category
- Never invent categories â€” use only those that exist on the wiki

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
wikitool templates show "Template:Infobox person"
wikitool templates examples "Template:Infobox person" --limit 2
wikitool templates show "Template:Cite web"
```

This reads the local template catalog from your current pull. If the local index is missing, run `wikitool knowledge build` first; if the catalog is missing, run `wikitool templates catalog build`.

---

## 7. Looking up templates and extensions

### Template context

Use wikitool to inspect template context from your local pull:

```bash
wikitool knowledge article-start "Topic Title" --format json
wikitool knowledge pack "Topic Title" --format json
wikitool knowledge pack --stub-path wiki_content/Main/Topic_Draft.wiki --format json
wikitool templates show "Template:Template Name"
wikitool templates examples "Template:Template Name" --limit 2
wikitool wiki profile show --format json
wikitool search "infobox"
```

This is always authoritative â€” it reflects what's actually deployed on the wiki.

### Extension documentation

Extension docs are imported from mediawiki.org and searched locally:

```bash
wikitool docs search "embed video"             # Search imported docs
wikitool docs list                              # List all imported docs
wikitool docs import ExtensionName              # Import new extension docs
wikitool docs update                            # Refresh all imported docs
```

See `extensions.md` for a quick reference of the most-used content tags.

### Categories

```bash
wikitool search "Category:"                    # Browse categories
```

---

## 8. Reference files

| File | Purpose | When to read |
|---|---|---|
| `style_rules.md` | Natural writing antipatterns | **Before every article** |
| `article_structure.md` | Structural template | Before writing new articles |
| `extensions.md` | Quick reference for content extension tags | When using math, code, video, tabs |

For template parameters and categories, always use wikitool live lookups rather than static files.
