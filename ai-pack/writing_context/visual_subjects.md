# Writing Visual Subjects

Guidance for articles whose subject is primarily a visual work: artworks, illustration and character
designs, PFP/NFT art collections, card art, logos, generative output, comics, and design lines. Read
`style_rules.md`, `article_structure.md`, and `writing_guide.md` first; this file adds the rules that
those general docs do not cover.

Most wiki writing guidance assumes prose about people, organizations, and events, where important
claims need independent coverage. Remilia Wiki is different from Wikipedia in one important way: it
often preserves niche subcultural material before anyone else has written it down. Primary sources,
hosted artifacts, quality-gated human knowledge, and the wiki's own source record may be the first
durable tether to the history. The standard is article quality and source integrity, not externality
for its own sake.

Visual subjects add one specific primary-source lane: the work itself is a primary source you are
allowed to describe. The skill is knowing exactly where description ends and interpretation begins.

---

## 1. Core principle: the work is a primary source you may describe

A hosted image, card, or artwork on the wiki is a primary document. You may describe what is plainly
and verifiably visible in it, the same way an art article describes a painting it depicts. You do not
need a secondary source to state that a figure has wings or that a palette is mostly gold.

Two conditions make this legitimate:

1. The work is shown in the article (lead image, infobox image, or gallery), so a reader can verify
   the description against the artifact.
2. The description is something independent viewers would agree on without specialist knowledge.

If you have not actually viewed the work, you cannot describe it. Do not convert a user's verbal
description of an image into article prose as if you had seen it. View the file first (the wiki hosts
it), or attribute and defer.

## 2. The describe / interpret boundary

This is the central discipline for visual subjects.

| You MAY assert from the work itself | You NEED a separate source, quality-gated statement, or omit |
|---|---|
| Subject matter ("a winged figure in armored boots") | Intent ("designed to evoke fragility") |
| Composition, pose, framing, recurring motifs | Influence and lineage ("inspired by Touhou") |
| Color, palette, contrast | Meaning, symbolism, what it "represents" |
| Medium when unambiguous (marker, pixel art, 3D) | Resemblance claims ("echoes Milady Maker") |
| Format and on-artifact text (titles, labels, captions) | Reception, popularity, significance |
| Series-level consistency across multiple works | Authorship of individual pieces (unless credited) |
| Counts you can see ("six cards are shown") | Totals you cannot see ("the full roster is N") |

The right-hand column is where AI-written art articles fail. Influence, intent, resemblance, and
meaning feel like description but are interpretation; they require a source that actually made the
claim. A comparison ("reminiscent of X") needs a cited critic who drew it, never the agent's own eye.
See the notability-name-dropping rule in `style_rules.md`.

When the creator states intent or lineage in an interview, that is a lead, not license to present it
as independently sourced article prose. Route it as in section 4.

## 3. Sourcing and showing the work

- Show before you describe. Place a representative work as the lead image and use a `<gallery>` for a
  series. Description in the body should be checkable against what is shown.
- Captions name and locate; they do not interpret. "Skull Bug Girl, the Halloween 2025 card" is good;
  "the hauntingly beautiful Skull Bug Girl" is not.
- On-artifact text is quotable primary evidence. If a card prints a name and a species binomial, you
  may report that pairing because it is legible in the image.
- Leave licensing, archive, and screenshot fields for human editors, as with all citations.
- Technical facts that are not visible (release date, tool used, supply, contract address) still need
  a normal secondary or primary source; the image does not establish them.

## 4. Interviewing for visual subjects

Visual subjects are the case where the creator usually knows far more than any public source. The
interview is where that knowledge is captured, but capture is not publication.

- Elicit the visual lexicon: how the line is made, the stated influences, the design rationale, the
  recurring motifs, what a viewer is meant to notice. Record it in the brief's Interview Transcript
  and Context.
- Separate the seen from the said. "The cards show pointed ears" is corroborated by the artifact.
  "The ears reference Milady" is the creator's reading; log it as a `do_not_assert` open item until
  sourced.
- Offer the durable-primary-record lane when it would improve verification. The richest material is often uncitable only because it was
  never made publicly inspectable. The correct fix is not to assert it anyway, and not to silently
  drop it, but to create or locate a durable record: a collection page, a post, a design note, a
  wiki source note, an archived primary artifact, or a creator-published statement that
  can be cited or otherwise inspected. Log this as a `missing-source` open item so a later session
  can close it.
- Decide the artifact-description boundary explicitly with the user. Some wikis and editors are
  comfortable with rich primary-artifact description; others want strictly secondary-sourced prose.
  This choice materially changes article length; make it on purpose, not by default.

## 5. Structure patterns for visual subjects

Adapt these to the subject; not every article needs every section. Each carries a sourcing note.

| Section | Typical content | Sourcing |
|---|---|---|
| Lead | What the work or series is, by whom, when, and why it exists | Definition; cite the establishing source |
| Design and aesthetic | Visible style, motifs, medium, series consistency | Primary-artifact description, anchored to shown images |
| Production / technique | How it was made, tools, process, who made it | Needs a source; do not infer process from the image alone |
| Series / roster | Named works or variants, seasonal or limited entries | Visible entries from artifacts; totals need a source |
| Influence and lineage | Stated influences, traditions, relationships | Separate source, attributed creator statement, or quality-gated human statement |
| Reception | Coverage, response, notable use | Secondary sources only; never the agent's judgment |

Thinness test: if "Design and aesthetic" can only restate one sourced sentence, the subject may not
yet support a standalone article. Prefer a redirect plus a section in the parent until a fuller source
exists, rather than padding with interpretation. A short, honest article beats a long, speculative one.

## 6. Art-writing anti-patterns

In addition to the general antipatterns in `style_rules.md`:

- Art-criticism puffery: "striking", "evocative", "masterful", "intricate", "haunting", "stunning".
  Describe what is there; let the reader judge.
- Unsourced influence: "draws on", "in the tradition of", "reminiscent of", "pays homage to". Each is
  an interpretive claim that needs a citation.
- Intent attribution: "meant to", "designed to evoke", "intended to symbolize" without a source.
- Over-description: cataloguing every pixel. Describe the load-bearing, recurring, or distinctive
  features, not an exhaustive inventory.
- Inferred process: stating how a work was made from how it looks. Medium may be visible; workflow,
  tools, and authorship usually are not.

## 7. Remilia specifics

- Post-authorship: attribute series to `parent_group = Remilia`, not an individual artist, unless a
  source credits a person. Discuss named contributors in the body when sourced.
- Visual lexicon: terms like network spirituality, dollcore, oekaki, and gijinka may be used as
  established terminology, but a claim that a specific work belongs to or derives from one of these
  traditions is an interpretive claim that needs a source, an attributed creator statement, or
  quality-gated human knowledge.
- Many Remilia subjects are art-first and recent. Expect human-provided creator knowledge to be the
  norm; durable primary records, target-wiki source notes, and primary-artifact description are the
  honest ways to raise quality without pretending broader coverage exists.

## 8. Worked example: beetle girls

The beetle girl cards in Beetleboy illustrate the boundary:

- Assertable from the cards (shown in a gallery): each card anthropomorphizes a beetle species; the
  figures have pointed ears, large eyes, and insect wings; the Skull Bug Girl card prints the name and
  the binomial "Eucorysses grandis"; costuming is themed to the species.
- Assertable from the design-notes source: the cosplay concept, the monster girl and mecha girl
  framing, the oekaki marker style.
- Leads only, not assertable without a source: the gijinka and mecha-musume design rationale, the
  Touhou influence, the intended Milady resemblance, the total roster size. These belong in the brief
  as `do_not_assert` open items, with a `missing-source` open item proposing a published collection
  page.

The result is a shorter article than the creator's full knowledge would suggest, and that is correct
until the leads are published somewhere citable.

## 9. Quick checklist

1. Did you actually view the work, or are you repeating a verbal description?
2. Is every visual claim verifiable from an image shown in the article?
3. Did any influence, intent, resemblance, or meaning claim slip in without a source?
4. Are captions descriptive rather than interpretive?
5. Did you route uncitable creator knowledge to leads, `do_not_assert` open items, and a
  source-note or creator-statement path, rather than asserting or silently dropping it?
6. If "Design and aesthetic" is one sentence, should this be a redirect plus a parent section instead?
