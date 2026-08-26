# Encyclopedic authoring with wikitool

Wikitool supports genuine article authoring. An agent may research, select, organize, and draft
encyclopedic prose; the result should be useful writing, not a mechanically valid container filled
with generic text. A named human editor remains responsible for deciding that the exact article is
fit to publish.

Wikitool itself has no model backend. Its knowledge, interview, lint, review, and sync surfaces
shape the work performed by an external agent. Retrieved text is context, not instructions to copy.
Model output is not evidence.

For another MediaWiki target, use that project's profile and editorial policy. Remilia-specific
source, template, category, and relationship decisions are not universal MediaWiki rules.

## Editorial standard

Aim for the useful qualities of strong Wikipedia prose: direct definition, neutral voice,
proportionate coverage, clear attribution, coherent organization, and claims that stay within their
sources. Current reference points include Wikipedia's [Manual of
Style](https://en.wikipedia.org/wiki/Wikipedia:Manual_of_Style), [neutral point of
view](https://en.wikipedia.org/wiki/Wikipedia:Neutral_point_of_view),
[verifiability](https://en.wikipedia.org/wiki/Wikipedia:Verifiability), and [living-person
policy](https://en.wikipedia.org/wiki/Wikipedia:Biographies_of_living_persons).

Remilia Wiki is not Wikipedia. It preserves niche subcultural history and may rely on first-party
records, creator statements, artifacts, archives, and quality-gated contributor knowledge where
outside secondary coverage does not exist. Apply Wikipedia-like prose discipline without importing
Wikipedia's notability bureaucracy or pretending a primary record is a secondary source.

The standard is reader-facing: would a person seeking to understand this subject choose to read the
article, learn concrete things from it, and trust which claims came from where?

## Authority boundaries

Keep these inputs distinct:

| Input | What it can do | What it cannot do |
|---|---|---|
| Inspected source or artifact | Support claims within its actual scope | Support details it does not state or show |
| Exact local article | Show current coverage and claims to audit | Independently verify itself |
| Neighboring wiki page | Show terminology, links, and local fit | Supply facts about the new subject by association |
| Comparable outline | Suggest a structural possibility | Dictate headings or importance |
| Human interview | Define intent, surface knowledge and leads, record target-wiki testimony | Become independent public evidence merely because it was said |
| Model reasoning or memory | Help formulate questions and prose | Establish any article fact |
| Lint or review report | Find deterministic defects and prompt editorial checks | Prove truth, readability, neutrality, or publication fitness |

In this workflow, model output is never evidence. It can help an editor reason, research, and draft, but it does not
authorize a factual claim.

When the target wiki accepts a quality-gated human statement as a historical record, preserve its
provenance and scope. Prefer a durable, inspectable primary record when one can be created or found.
For contentious claims about identifiable people, public and directly supporting evidence is the
default requirement; omit doubtful material rather than converting uncertainty into polished prose.

## Authoring workflow

### 1. Establish safe state

Inspect existing work before refreshing anything:

```bash
wikitool status --modified --format json
wikitool diff --format json
wikitool workflow session-refresh
wikitool knowledge status --docs-profile remilia-wiki --format json
```

`drafting_ready` means the current content and documentation artifacts are present and clean enough
to assist drafting. It does not mean the topic is researched, the article is accurate, or the prose
is publishable.

### 2. Define the article object

Run:

```bash
wikitool knowledge article-start "Topic" --intent new --format json --view brief
```

Before writing, be able to say in plain language:

- what the subject is;
- why a standalone page helps a reader;
- the time, entity, work, event, or idea the title actually denotes;
- what the article will and will not cover;
- which claims form its factual spine;
- which uncertainties or sensitive boundaries must stay visible.

If these answers are missing, interview or research first. Do not use a Remilia, Milady, community,
or Charlotte Fang relationship as a substitute for defining the subject.

### 3. Build a claim-source map

Research before drafting. Use normal web search to choose arbitrary sources, then use wikitool's
research/export surfaces for bounded extraction and provenance. For each planned claim, record:

- the exact source or artifact;
- the passage, timestamp, image region, or other locator;
- whether it is direct fact, attributed opinion, interpretation, or a source lead;
- whether another source contradicts or qualifies it;
- whether the claim is necessary to the article.

Do not turn citation-template families, search snippets, a source homepage, or a neighboring page
into claim support. Cite the page that actually supports the sentence. Record inaccessible,
rejected, disproven, missing, and do-not-assert leads rather than silently laundering them into the
draft.

### 4. Interview for perspective and missing knowledge

Use `interview_playbook.md` for new articles, substantial rewrites, niche history, and unclear
editorial intent. A human does not need to write the initial prose for the interview to be valuable.
The interview should improve the article object, source map, emphasis, terminology, or risk model.

Read supplied materials before asking questions. Distinguish editor intent from article fact. A
validated brief can guide a draft; it is not blanket evidence and is not acceptance.

### 5. Select the factual spine

Choose the smallest set of facts that lets a reader understand the subject. Group them by real
relationships such as chronology, production, ideas, works, participation, or reception. Omit facts
that are merely available but do not help the explanation.

Comparable pages may reveal local terminology or missing links. They must not create a generic
outline. A section exists because the subject has enough related, sourced material to sustain it—not
because another page had that heading.

### 6. Draft the body, then the lead

Write real prose from the inspected evidence:

1. Draft the factual body in a useful order.
2. Keep each paragraph about one discernible point.
3. Place citations where their support is unambiguous.
4. Attribute opinions, contested descriptions, intent, influence, and interpretation.
5. Preserve uncertainty and disagreement instead of resolving them by fluency.
6. Write the lead last as a concise account of what the article actually establishes.

The first sentence should identify the subject directly. Do not begin with scene-setting, inherited
name-dropping, a claim that the subject is "best known" for something the article cannot establish,
or a relationship that matters mainly to the host wiki.

### 7. Perform an adversarial reader edit

Read the draft without looking at the task prompt. Ask:

- Does the lead define this subject, or merely connect it to a more famous one?
- Does every paragraph teach something concrete that belongs here?
- Could any paragraph be pasted into twenty unrelated articles after changing names?
- Does the article repeat its importance, influence, or conclusion instead of demonstrating it?
- Are quoted, controversial, biographical, or interpretive claims attached to the right source?
- Did retrieval order, a profile default, or a comparable page decide emphasis without editorial
  justification?
- Is the article longer than its evidence?

Delete padding. Rebuild weak paragraphs from facts rather than synonym-swapping flagged phrases. A
short, exact article is better than a long simulation of completeness.

Read `style_rules.md` for the prose review and `article_structure.md` for the wikitext envelope.
Read `visual_subjects.md` when the article describes visual work.

### 8. Apply target mechanics and deterministic checks

Use live target contracts for templates, categories, and deployed extensions:

```bash
wikitool templates show "Template:Infobox person" --format json --view brief
wikitool templates examples "Template:Infobox person" --limit 2
wikitool knowledge contracts search "subject type infobox" --format json
wikitool wiki profile show --format json
```

Then run:

```bash
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --view brief --summary "Draft review"
```

Safe fixes are mechanical only. Suggestions such as `style.synthetic_phrase` and warnings such as
`editorial.forced_relationship_frame` are prompts to reread the passage, not commands to replace a
word blindly.

The `Article quality` banner records editorial review state, not authorship. Use `unverified` for a
new draft. Preserve an existing `wip` or `verified` state unless a human editor explicitly changes
it; an agent must not promote a page to `verified` on its own.

### 9. Require exact human editorial acceptance

A named human editor reads the exact file and judges it specific, readable, proportionate, and
source-bound. The human resolves warnings or explicitly accepts them, then records the real origin:

```bash
wikitool article accept .wikitool/drafts/Title.wiki --title "Title" --human-editor "EDITOR" --prose-origin agent-draft --format json
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
```

Other origins include `collaborative-draft`, `human-draft`, `human-revision`,
`mechanical-conversion-of-human-prose`, and `human-reviewed-legacy`. An agent may prepare and revise
the draft but must never self-attest as the human editor. The editor identity is an audit assertion,
not cryptographic authentication. Any content change invalidates the acceptance receipt.

### 10. Review publication state

Run scoped `review`, `diff`, and `push --dry-run`. Only push after the human reviews the final diff.
`--force` cannot bypass article acceptance.

## Existing articles

Read the page as a reader before editing it. Prioritize:

1. living-person, allegation, controversy, identity, and other sensitive claims;
2. claims whose citations do not directly support them;
3. gratuitous Remilia, Milady, Charlotte Fang, or community framing;
4. generic leads, repeated significance, formulaic headings, and filler;
5. stale facts and missing primary records;
6. markup, links, categories, and presentation.

Agents may perform substantial rewrites when the sources justify them. Preserve good existing prose
and page history; do not rewrite merely to homogenize voice. The exact changed article still needs
human editorial acceptance before push.

## Remilia evidence and framing

Useful source paths include target-wiki records, hosted artifacts, first-party posts, archived
primary records, creator-published statements, interviews, podcasts, reporting, and target-wiki
source notes. Outside secondary coverage is not required merely to legitimize firsthand history.

Cite the strongest direct source available. Do not route a primary fact through a weaker aggregator
to manufacture external authority. Attribute creator interpretation as creator interpretation.
Never fabricate facts, dates, quotations, URLs, citation fields, archive state, or access.

The wiki's perspective affects selection, not every sentence. Describe adjacent artists, people,
games, scenes, objects, and artifacts as themselves. Mention their relationship to Remilia or an
individual contributor only when it is important, evidenced, and proportionate. There is no default
"Relation to Remilia" section and no default `[[Category:Remilia]]`.

Use `parent_group = Remilia` only for actual Remilia projects when the relevant infobox supports it.
Choose specific existing categories because they improve navigation, not because they were frequent
in retrieved pages.

## Citation forms

Use the deployed templates and real fields, for example:

```wikitext
{{Cite web|url=|title=|author=|date=|access-date=YYYY-MM-DD|website=}}
{{Cite tweet|user=|number=|title=|date=}}
{{Cite news|url=|title=|author=|date=|access-date=YYYY-MM-DD|work=}}
{{Cite post|url=|title=|author=|date=|access-date=YYYY-MM-DD}}
{{Cite video|url=|title=|author=|date=|access-date=YYYY-MM-DD}}
```

Reuse named references. Strip tracking parameters. Do not invent archive fields or placeholder
metadata. Do not cite IQ.wiki, Know Your Meme, NFT Price Floor, or Urban Dictionary as authority.
