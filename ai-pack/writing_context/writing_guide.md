# Remilia Wiki — Writing Guide

You create encyclopedic articles for a MediaWiki wiki about Remilia Corporation, Milady Maker, and the network spirituality ecosystem.

**Wiki:** https://wiki.remilia.org
**Stack:** MediaWiki 1.44 with Lua templates, Cargo structured data, CirrusSearch

This guide is the bundled default writing profile for Remilia Wiki. For another MediaWiki target,
prefer the host project's `writing_context/` and the target wiki's live wikitool profile/template
surfaces; do not apply Remilia-specific sourcing, category, or template rules as universal
MediaWiki behavior. Read `style_rules.md` before every article.

---

## 1. Output format

All output must be raw MediaWiki wikitext, ready for direct use on the wiki. Never output Markdown. Never wrap output in code blocks. Never include commentary or meta-text — only article wikitext.

---

## 2. Article workflow

### Writing a new article

1. **Read `style_rules.md`** — internalize the antipatterns before writing.
2. **Refresh local authoring state** - run `wikitool status --modified --format json`, `wikitool diff --format json`, `wikitool workflow session-refresh`, and `wikitool knowledge status --docs-profile remilia-wiki --format json` so local changes, content, templates, docs readiness, and capability signals are current. Use `wikitool workflow full-refresh` only for a deliberate rebuild or missing sync state, and do not use `pull --overwrite-local` unless the user explicitly approves discarding local edits.
3. **Build the interpreted authoring brief** - run `wikitool knowledge article-start "<Topic>" --intent new --format json --view brief`. This is the front door. The `section_skeleton` shows which sections comparable pages use; `content_backed` flags tell you which sections already have evidence in the pack. For sections where `content_backed` is `false`, use `wikitool knowledge inspect chunks --view brief` to fetch targeted content before writing. When your own subject knowledge suggests a different wiki-contract lookup than the title itself, make that visible with `--contract-query`, such as `wikitool knowledge article-start "Cheetah" --contract-query "species infobox taxonomy" --format json --view brief`.
4. **Interview for human context when useful** - interviewing is an optional, conversational lane, not a required step. For new articles and substantial expansions, reach for `interview_playbook.md` after the scout when the user's own knowledge would improve scope, chronology, terminology, or sourcing; skip it freely and draft from sources when the subject is well-covered or the user prefers. Read any supplied documents, links, notes, transcripts, screenshots, or source excerpts before narrowing the questions. Start with a broad freeform prompt about what the subject is, why it matters, what sources or artifacts matter, what outsiders misunderstand, and what should not be overstated. Reflect the emerging article scope in neutral wiki language and ask adaptive follow-ups while the answers improve structure, research targets, terminology, chronology, source strategy, or risk. Before drafting, critique the emerging article plan and ask another round if it would otherwise be thin, duplicative, unprovenanced, wrongly framed, or missing the user's actual knowledge. Save reusable distillations under `.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Briefs are working notes, not article prose or finished citation evidence; treat a quality-gated human statement as reasonable encyclopedic truth, and cite when research surfaces a real source or the claim is the kind that needs one.
5. **Fetch external evidence selectively** - use normal agent web search to choose source URLs, then run `wikitool research fetch "<URL>" --output json` only for sources you expect to cite. Use `wikitool research wiki-search "<Topic>" --format json` only when you need configured target-wiki API results, not open-web search. If fetch output has `status: "error"`, treat it as a source-access failure; inspect `error.challenge_handoffs`, `error.discovery`, or run `wikitool research discover "<URL>" --format json` for public robots, sitemap, feed, and structured-data leads. When `error.challenge_handoffs` is present, relay the handoff to the user; if they have lawful browser access, they can solve the challenge and import source-issued cookies with `wikitool research session import "<URL>" --cookies -`, then you can retry with `--refresh`. Do not use stealth clients, TLS impersonation, paid crawlers, or third-party reader services. For source MediaWiki pages whose template contract matters, use `wikitool research mediawiki-templates "<URL>" --format json`; this describes the source wiki, not which templates are valid on the target wiki. Add `--refresh` when live freshness matters. Use `wikitool wiki profile remote "<URL>" --format json` only when you need a remote target wiki capability probe and local target profile/import data is unavailable. Do not cite challenge pages, blocked fetches, or fetch diagnostics as article evidence.
6. **Look up templates and profile rules** — use `wikitool templates show "Template:Template Name" --format json --view brief`, `wikitool templates examples "Template:Template Name" --limit 2`, and `wikitool wiki profile show --format json`.
7. **Write the article** following the structure in `article_structure.md`.
8. **Save** to `wiki_content/Main/{Article_Title}.wiki`.
9. **Run article-aware lint** — `wikitool article lint wiki_content/Main/{Article_Title}.wiki --format json`. If the fixes are purely mechanical, follow with `wikitool article fix wiki_content/Main/{Article_Title}.wiki --apply safe`. For large reference cleanups, use `wikitool knowledge inspect references summary --title "{Article_Title}" --format json` and `wikitool knowledge inspect references duplicates --title "{Article_Title}" --format json`.
10. **Review** — run `wikitool review --format json --view brief --summary "Summary"` before push. Use `wikitool validate --summary` for the lower-level global integrity signal and scoped validation flags when investigating a specific issue.

Use `wikitool knowledge contracts search "contract terms" --format json` for a direct token-budgeted search of the template/module graph before deciding which template or module to expand.

Keep retrieval token-tight. Prefer wikitool brief views and targeted `knowledge inspect chunks --view brief` calls for missing sections. Increase `--token-budget`, use broad `--across-pages`, or request `--view full` only after the compact brief identifies a specific gap.

### Editing an existing article

1. Refresh latest wiki state: `wikitool workflow session-refresh`
2. Read the existing article.
3. Make changes following all the same rules.
4. Lint the draft: `wikitool article lint wiki_content/Main/{Article_Title}.wiki --format json`
5. Review: `wikitool review --title "{Article_Title}" --format json --summary "Summary"`
6. Diff: `wikitool diff --title "{Article_Title}"`
7. Preflight conflicts: `wikitool push --dry-run --title "{Article_Title}" --summary "Summary"`
8. Push: `wikitool push --title "{Article_Title}" --summary "Summary"`

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
For many Remilia subjects, the wiki may be the first durable record of niche internet history. Do
not discard important creator/editor knowledge merely because no outside publication exists. Do
make the provenance inspectable: cite or anchor claims to first-party posts, target-wiki records,
hosted artifacts, archived primary records, or creator/editor-published statements, and attribute
interpretive claims when they come from a creator rather than from the artifact itself.

Remilia Wiki is not only a catalog of things directly about Remilia. Treat it as the online world
viewed from Remilia's perspective. Adjacent artists, games, scenes, objects, and artifacts should be
written as themselves when they are worth canonicalizing in that field of view. Do not force a
"relationship to Remilia" section or lead frame unless the relationship is real, sourceable or
editor-attested, and article-shaping. Do not create generic article sections such as "Editorial
vantage" or "Why this belongs here"; use that reasoning to choose article boundaries, not as prose.

**Good sources:**
- Official announcements, blog posts, project websites
- Target-wiki pages, hosted files, galleries, and source notes when they are the durable primary record
- Creator/editor-published statements, clearly attributed
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

Articles marked `{{Article quality|verified}}` represent editor-reviewed content. Use them to:
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

### Citation integrity and first-party facts

This is a subcultural wiki: treat a quality-gated statement from the creator or a knowledgeable
editor as reasonable encyclopedic truth, the same way a game or fandom wiki records its own subject.
Do not demand outside secondary coverage for first-party or subcultural facts the editor knows. Cite
when research surfaces a real source, when a claim is external, contested, or surprising, or when a
primary record exists (an on-chain value, a dated post, a hosted artifact) - and when you cite, cite
that actual source, not a third party that merely restated it. Reserve attribution ("According to
...") for disputed or interpretive claims. See "Source laundering" in `style_rules.md`: never route a
primary fact through a weaker aggregator to manufacture an external citation.

### Interview briefs and user knowledge

Interview briefs under `.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md` preserve
human context from authoring sessions. They can widen research and improve article structure, but
they are not article prose, citation evidence, or proof that the interview is complete. A
quality-gated human statement can become article prose as reasonable truth; cite it when research
surfaces a source or when the claim is external, contested, or the kind that needs one, and anchor it
to a primary record (first-party post, target-wiki record, hosted artifact, archived primary record,
or creator/editor-published statement) when one exists. Record what you deliberately could not
source, or should not assert yet, as an open item so a later session does not rediscover or silently
assert it. Mechanical validation of a brief means the ledger can be used; it does not mean the
article is ready.

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
- Check if target exists: `wikitool research wiki-search "Article Name" --what title --format json`
- Never place red links in See also sections
- Use piped links when display text differs: `[[Remilia Corporation|Remilia]]`

### Quality marking

Every new agent-authored main-namespace draft should include `{{Article quality|unverified}}` on
line 2 as the default editorial review state. Preserve an existing `wip` or `verified` state unless
the user explicitly asks to change it. Agents should not promote an article to `verified` on their
own; that state means the article has been accepted through the wiki's editorial review process.

### Categories

Categories are managed via the wiki database. To find valid categories:

```bash
wikitool research wiki-search "Category:" --what title --format json       # List/browse categories
wikitool research wiki-search "Category:Name" --what title --format json   # Search specific category
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
wikitool templates show "Template:Infobox person" --format json --view brief
wikitool templates examples "Template:Infobox person" --limit 2
wikitool templates show "Template:Cite web" --format json --view brief
```

This reads the local template catalog from your current pull. If the local index is missing, run `wikitool knowledge build` first; if the catalog is missing, run `wikitool templates catalog build`.

---

## 7. Looking up templates and extensions

### Template context

Use wikitool to inspect template context from your local pull:

```bash
wikitool knowledge article-start "Topic Title" --format json --view brief
wikitool knowledge article-start "Topic Title" --contract-query "subject type infobox" --format json --view brief
wikitool knowledge contracts search "subject type infobox" --format json
wikitool templates show "Template:Template Name" --format json --view brief
wikitool templates examples "Template:Template Name" --limit 2
wikitool wiki profile show --format json
wikitool knowledge inspect chunks --across-pages --query "infobox" --limit 10 --token-budget 1200 --format json
```

This is always authoritative — it reflects what's actually deployed on the wiki.

### Extension documentation

Extension docs are imported from mediawiki.org and searched locally:

```bash
wikitool docs search "embed video"             # Search imported docs
wikitool docs list                              # List all imported docs
wikitool docs import ExtensionName              # Import new extension docs
wikitool docs update                            # Refresh all imported docs
```

See `extensions.md` for a quick reference of the most-used content tags.

For local/custom features such as Remilia's current D3Charts bridge, prefer target-wiki evidence:
`wikitool wiki profile show --format json`, `wikitool knowledge contracts search "d3 chart" --format json`,
`wikitool templates show "Module:D3Chart" --format json --view brief` where available, and `wikitool article lint`. Do not add
inline JavaScript or raw generated HTML to article wikitext; use the deployed module or extension
contract, and expect that a future bespoke extension may supersede the current `Module:D3Chart` form.

### Categories

```bash
wikitool research wiki-search "Category:" --what title --format json       # Browse categories
```

---

## 8. Reference files

| File | Purpose | When to read |
|---|---|---|
| `style_rules.md` | Natural writing antipatterns | **Before every article** |
| `article_structure.md` | Structural template | Before writing new articles |
| `visual_subjects.md` | Art, character, and visual-subject writing rules | When the subject is a visual work |
| `extensions.md` | Quick reference for content extension tags | When using math, code, video, tabs |

For template parameters and categories, always use wikitool live lookups rather than static files.
