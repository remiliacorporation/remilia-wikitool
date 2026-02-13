# Natural Writing Style Rules

Rules for writing encyclopedic articles that read as human-authored prose.
Derived from Wikipedia's Manual of Style and Signs of AI Writing.

These are **hard rules**, not suggestions. Violating them produces text that reads as machine-generated.

---

## I. Banned phrases and constructions

### Significance inflation

Never attach importance-claims to mundane facts. State what happened; let the reader judge significance.

**Never use:**
- "stands as", "serves as a testament", "is a testament to"
- "pivotal role", "crucial role", "significant role", "key role"
- "key moment", "key turning point", "watershed moment"
- "underscores its importance", "highlights its significance"
- "reflects broader trends", "reflects the evolving"
- "symbolizing its ongoing", "enduring legacy", "lasting impact"
- "contributing to the", "setting the stage for"
- "marking/shaping the", "represents a shift", "marks a shift"
- "evolving landscape", "cultural landscape"
- "focal point", "indelible mark", "deeply rooted"
- "profound heritage", "rich history"

**Instead:** State specific facts. "The collection sold 10,000 units in 48 hours" is better than "The collection played a pivotal role in shaping the evolving NFT landscape."

### Promotional puffery

**Never use:**
- "rich tapestry", "vibrant tapestry", "diverse tapestry"
- "groundbreaking", "revolutionary", "trailblazing"
- "stunning", "breathtaking", "fascinating glimpse"
- "boasts a", "continues to captivate"
- "nestled in", "in the heart of"
- "intricate", "robust", "comprehensive" (when vague)
- "innovative", "cutting-edge"

### Copula avoidance

AI text systematically avoids "is/are/was/has" in favor of elaborate substitutions. Use the simple form.

| Bad (AI pattern) | Good (natural) |
|---|---|
| serves as the headquarters | is the headquarters |
| features a collection of | has a collection of |
| boasts a population of | has a population of |
| marks the beginning of | is the beginning of |
| stands as an example of | is an example of |
| represents a departure from | is a departure from |
| offers a glimpse into | shows |
| ventured into politics as | was a candidate |

### AI vocabulary (statistically overused)

These words appear at dramatically higher rates in AI text than in human writing. Any single use may be fine, but clustering them is a strong AI signal.

**Watch list — use sparingly or not at all:**
- additionally, furthermore, moreover, notably
- particularly, specifically, essentially, fundamentally
- multifaceted, comprehensive, intricate, nuanced
- landscape, tapestry, realm, sphere
- leverage, harness, foster, facilitate
- delve, underscore, showcase, highlight
- testament, cornerstone, hallmark
- encompasses, embodies, encapsulates
- commendable, noteworthy, remarkable

**Prefer instead:** plain connectives (and, but, also, however), concrete verbs, specific nouns.

### Didactic disclaimers

**Never use:**
- "it's important to note", "it should be noted"
- "it is crucial to differentiate", "it is worth mentioning"
- "it's important to remember"
- "may vary", "results may differ"
- "it is beyond the scope of"

These address the reader directly. Encyclopedic prose never does this.

### Section summaries and conclusions

**Never:**
- Add a "Conclusion" section
- End sections with "In summary," / "In conclusion," / "Overall,"
- Restate a section's main point at its end
- Add a "Challenges and future outlook" section using the formula: "Despite its [positive words], [subject] faces challenges..."

Sections should end with the last relevant fact, not a wrap-up.

### Elegant variation

Do not strain to find synonyms for the subject. Repeating the name or using "it/they" is better than cycling through "the project", "the collection", "the initiative", "the venture".

**Bad:** "Milady Maker launched in 2021. The collection... The project... The PFP initiative..."
**Good:** "Milady Maker launched in 2021. It... The collection..." (pick one and stay consistent)

### False ranges

Only use "from X to Y" when X and Y sit on a real, identifiable scale (time, quantity, severity).

**Bad:** "from problem-solving to artistic expression" (no scale)
**Bad:** "from fundamental physics to medicine" (different fields, not a range)
**Good:** "from 2019 to 2023" (time scale)
**Good:** "from mild to severe" (severity scale)

### Negative parallelisms

**Never use argumentative constructions:**
- "Not only X, but also Y"
- "It is not just about X, it's about Y"
- "No mere X, but a Y"

These are persuasive, not encyclopedic. State both facts plainly.

### Rule of three

Do not artificially group items in threes for rhetorical effect.

**Bad:** "keynote sessions, panel discussions, and networking opportunities"
**Good:** Name the specific sessions, or describe what happened.

### Superficial analysis via participle phrases

Do not attach "-ing" phrases that assign significance to facts.

**Bad:** "The exhibition opened in March, highlighting the growing interest in digital art."
**Bad:** "The project launched on Ethereum, showcasing the possibilities of on-chain art."
**Good:** "The exhibition opened in March." (Let the reader draw conclusions.)

---

## II. Prose style

### Write plainly
- Use short sentences mixed with medium ones. Avoid uniformly long sentences.
- Prefer active voice. "Remilia launched the collection" not "The collection was launched by Remilia."
- Use concrete verbs: "sold", "created", "published", "wrote" — not "leveraged", "facilitated", "showcased".

### Lead section
- The lead summarizes the entire article in 1-4 paragraphs.
- The first sentence defines the subject: **'''Subject'''** is/was [definition].
- The lead should stand alone as a useful summary.
- Do not add citations to the lead if the same facts are cited in the body.

### Flowing prose over lists
- Write paragraphs, not bullet points. Lists are for genuinely discrete items (tracklists, member rosters, dates).
- **Bad:** Converting three related facts into a three-item bullet list.
- **Good:** "The project's goals are to document history, create a community, and provide accurate information."

### Consistent terminology
- Pick one term for a concept and use it throughout. Don't alternate between "the project", "the collection", "the initiative" for the same thing.
- Use the subject's actual name, not elegant variations.

### Sentence case for headings
- `== Early life and education ==` (correct)
- `== Early Life and Education ==` (wrong — title case)
- Exception: proper nouns within headings retain their capitalization.

### No direct address
- Never address "you" or "the reader".
- Never use "we" to mean "you and I, the reader".
- Third person only.

---

## III. Formatting and markup

### Wikitext, not Markdown
- `'''bold'''` not `**bold**`
- `''italic''` not `*italic*`
- `== Heading ==` not `## Heading`
- `[[Link]]` not `[Link](url)`
- `----` not `---`
- Never output fenced code blocks (` ```wikitext `)

### Straight quotes only
- Use `"` and `'` (straight), never `"` `"` `'` `'` (curly).
- This applies to all text including quotations.

### Bold usage
- Bold the article title in the first sentence only: `'''Milady Maker''' is...`
- Never use bold for emphasis in the body. Use italics sparingly, or rewrite.

### Dashes
- Use em dashes (—) sparingly. Commas, parentheses, or colons are often better.
- Never cluster multiple em dashes in a paragraph.

### Citations after punctuation
- **Correct:** `...the event occurred.<ref>{{Cite web|...}}</ref>`
- **Wrong:** `...the event occurred<ref>{{Cite web|...}}</ref>.`

### Non-English text
- Mark non-English words with `{{lang|xx|word}}` where xx is the ISO 639 language code.
- Proper names in other languages typically don't need this.

### Images and infoboxes
- Place infoboxes and lead images right-aligned.
- Use `|thumb|right|` for article images.
- Write descriptive captions — they appear in popups and previews.

---

## IV. Citation hygiene

### Never fabricate sources
- Every URL, DOI, and ISBN must be real and verifiable.
- If you cannot find a source, do not make the claim.
- Never invent reference details to fill gaps.

### No URL trackers
- Strip `?utm_source=chatgpt.com`, `?utm_source=openai`, `?referrer=grok.com` and similar tracking parameters from all URLs.

### No placeholder content
- Never output: `[Author Name]`, `INSERT_SOURCE_URL`, `2025-XX-XX`, `https://www.example.com/source`
- If information is unavailable, omit the field entirely.

### No system artifacts
- Never output: `citeturn0search0`, `contentReference[oaicite:0]`, `oai_citation`, `({"attribution":{"attributableIndex":"..."}})`, `attached_file:1`
- Never output `[cite_start]`, `[cite: N]`, or similar citation markers from training data.

### Named references for reuse
- First use: `<ref name="shortname">{{Cite web|...}}</ref>`
- Subsequent: `<ref name="shortname" />`
- Never duplicate the full citation. Never declare named refs inside `{{Reflist}}`.

### Access dates
- Use today's date for `access-date`, not a date from weeks or months ago.
- All citations in one article should have consistent, current access dates.

### Archive fields
- Leave ALL archive fields blank: `archive-url`, `archive-is`, `archive-date`, `screenshot`.
- Human editors complete these manually later.

### Citation density
- Aim for 2-5 citations in a short article (2-4 paragraphs).
- Cite specific claims about the subject, not general background.
- **Bad:** "NFTs are digital tokens.<ref>1</ref> They are recorded on blockchains.<ref>2</ref>"
- **Good:** "NFTs are digital tokens recorded on blockchains. Milady Maker launched in August 2021.<ref>1</ref>"

---

## V. Meta-communication

### Never talk about the task
- No "Certainly!", "Here's a draft...", "I hope this helps", "Let me know if..."
- No "In this article, we will discuss..."
- No "As an AI language model..."

### Never mention knowledge limitations
- No "as of my last knowledge update", "based on available information"
- No "details aren't widely documented", "is limited in the provided search results"
- No speculation about gaps in sources.

### Never include reviewer notes
- No submission statements explaining why the article is notable.
- No notes to editors about what to check.
- Output only the article wikitext, nothing else.

---

## VI. Unreliable sources

**Never cite these — they appear in search results but are unreliable:**
- IQ.wiki (iq.wiki) — inaccurate, user-generated
- Know Your Meme (knowyourmeme.com) — tertiary source, quality issues
- NFT Price Floor — inaccurate details

If these are the only sources available, note the lack of reliable sourcing rather than citing them.

---

## VII. Quick self-check

Before finishing any article, verify:

1. **No banned phrases** — search your output for "tapestry", "landscape", "pivotal", "testament", "showcasing", "underscores", "highlights"
2. **Simple copulas** — did you use "is/was/has" or always avoid them?
3. **No section summaries** — does any section end with a restatement?
4. **Sentence case headings** — are all headings sentence case?
5. **Straight quotes** — any curly quotes?
6. **Clean citations** — any utm_source, placeholder dates, or fabricated URLs?
7. **No meta-commentary** — does output contain only article wikitext?
8. **Prose, not lists** — did you convert anything to bullets that should be prose?
