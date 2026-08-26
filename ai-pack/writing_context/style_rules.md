# Encyclopedic prose review

Use this review for human-, agent-, and collaboratively drafted articles. Judge the text that a
reader will see. Do not infer quality or authorship from a word list, and do not rewrite good prose
merely to make it look less machine-generated.

A lint phrase match is a reread signal, not a verdict. Passing lint is not evidence that an article
is accurate, neutral, readable, or worth publishing.

## 1. Hard failures

Do not publish prose with any of these defects:

- a factual claim rests on model memory, a search snippet, retrieval rank, or an unrelated local
  page rather than inspected evidence;
- a citation does not support the sentence it is attached to;
- quotation, date, identity, allegation, motive, influence, reception, or intent was invented or
  inferred beyond the source;
- a contentious claim about an identifiable person lacks direct, adequate support;
- the subject is defined mainly through an incidental relationship to Remilia, Charlotte Fang,
  Milady, or another better-known entity;
- the prose resolves uncertainty, disagreement, or missing evidence simply by sounding confident;
- a paragraph exists to make the article look complete rather than to tell the reader something.

## 2. Definition and scope

The lead's first sentence should answer “what is this?” in the most specific supported terms. It
should not answer “why is this on Remilia Wiki?” or “who is it connected to?” unless that connection
is genuinely part of the definition.

Bad:

> '''Example''' is best known in the Remilia community through its significant connection to
> Charlotte Fang.

Better pattern:

> '''Example''' is a [work, person, project, event, term, or organization] that [specific defining
> fact].

Only use the better pattern when the bracketed content is supported. Follow with the facts that the
article itself develops. Do not claim the subject is “best known” for something unless a source
actually establishes that public recognition.

Keep the scope stable. Do not slide between a person, their alias, a collective, a project, and a
community as though they are interchangeable. Define ambiguous names and changes over time.

## 3. Source fidelity

Write no more than the evidence allows.

- A source saying that an event occurred does not establish its importance.
- A creator describing an intention supports an attributed statement of intention, not an objective
  reading of the work.
- An artifact can support what is visibly or textually present; it usually cannot establish process,
  authorship, influence, date, or reception on its own.
- A homepage can establish only what the homepage actually says. Do not reuse it as a citation for
  multiple unrelated details.
- A local article is a claim to verify unless its underlying citation has been inspected.

Use direct attribution when the source owns the judgment: “The creator described…”, “According to
the announcement…”, or “The review characterized…”. Do not hide contested judgments behind “it is
considered”, “observers noted”, “critics say”, or other vague subjects.

## 4. Proportion and editorial selection

Coverage should reflect what is important to understanding the subject, not what happened to be
easy to retrieve.

- Give a relationship only the space its evidence and explanatory value warrant.
- Do not repeat the same importance claim in the lead, body, and final paragraph.
- Do not turn every available fact into a section.
- Do not pad thin evidence with background about the broader NFT, art, internet, or Remilia scene.
- Do not add “Impact”, “Legacy”, “Reception”, “Controversy”, or “Relation to Remilia” because the
  heading sounds encyclopedic. Add one only when distinct sourced material sustains it.
- A useful stub is an acceptable result. Apparent completeness is not.

## 5. Paragraph craft

Each paragraph should make one intelligible contribution and connect its sentences through facts,
not rhetorical glue.

Prefer:

- concrete nouns and verbs;
- plain forms such as “is”, “has”, “made”, “released”, and “said” when they are exact;
- chronology when sequence explains the subject;
- explicit causal language only when a source establishes causation;
- repeated use of the subject's actual name or a clear pronoun over strained synonyms.

Reread and usually remove:

- importance inflation: “pivotal”, “landmark”, “enduring legacy”, “testament to”, “underscores the
  significance”;
- promotional evaluation: “groundbreaking”, “visionary”, “stunning”, “vibrant”, “innovative”;
- empty transitions: “Moreover”, “Notably”, “In addition”, or “It is important to note” when the
  sentence has no real logical dependency;
- analytical-sounding participles that smuggle in a conclusion: “highlighting”, “showcasing”,
  “reflecting”, or “marking” without a source for the claimed significance;
- generic closing sentences that restate the paragraph or announce an “impact”;
- elegant variation that cycles through “the project”, “the initiative”, “the venture”, and “the
  collection” for one thing;
- formulaic contrasts such as “not merely X, but Y” and artificial three-item flourishes;
- vague hedges used instead of attribution or omission.

None of these words is mechanically banned. Keep one when it is the clearest accurate choice and
the sentence earns it. Rewrite the underlying thought, not just the flagged token.

## 6. Article-shaped filler

Delete a passage if changing the subject's name would let it fit many unrelated articles. Common
forms include:

- a scene-setting paragraph that explains a whole industry before identifying the subject;
- a “broader context” paragraph with no sourced connection to the subject;
- a conclusion that predicts continued relevance;
- a list of themes whose evidence is only the agent's interpretation;
- a section composed of one claim repeated in several phrasings;
- a paragraph that exists solely to mention a famous person or project.

Specificity is not extra detail for its own sake. It is the selection of names, dates, acts, works,
language, and relationships that actually distinguish this subject.

## 7. Leads

Write the lead after the body. It should stand alone as a compact account of what the article
establishes, normally one to four paragraphs depending on article length.

- Bold the article title on its first occurrence only.
- Define the subject directly in the first sentence.
- Summarize the body rather than introducing facts that appear nowhere else.
- Keep interpretation and reputation proportionate and attributed.
- Do not open with a quotation, rhetorical scene, host-wiki relationship, or importance claim.
- Cite sensitive or easily challenged lead claims even when they are also cited in the body.

## 8. Structure and flow

Use paragraphs for connected facts and lists for genuinely discrete data such as members,
tracklists, variants, or dates. Do not use bold pseudo-headings inside bullet lists.

Headings use sentence case and describe the material beneath them. If a heading would contain only
one small paragraph, consider merging it into a stronger section. End a section with its last useful
fact, not “In summary”, “Overall”, or a forecast.

## 9. Wikitext and citations

- Use wikitext, not Markdown: `'''bold'''`, `''italic''`, `== Heading ==`, and `[[Link]]`.
- Do not emit fenced code blocks around article text.
- Put citation markers after punctuation unless the target style requires otherwise.
- Reuse named references for the same source and claim family.
- Strip URL tracking parameters.
- Never emit placeholder sources, fake archive data, or model citation artifacts such as
  `citeturn0search0`, `oaicite`, `[cite_start]`, or `contentReference`.
- Use straight quotation marks in wikitext unless the target style specifies otherwise.
- Add bold only for the title in the lead, not as body emphasis.

## 10. Final reader test

Before acceptance, answer each question with the rendered article in mind:

1. What concrete understanding does the reader gain from every paragraph?
2. Is the subject intelligible without prior Remilia knowledge?
3. Does each citation support the claim nearest to it?
4. Are primary-source interpretation and human testimony attributed at the right level?
5. Did any retrieval artifact decide emphasis by accident?
6. Are sensitive claims necessary, proportionate, and strongly sourced?
7. Is any section present only because an encyclopedic article “usually has one”?
8. Can repeated significance language be replaced by a fact or deleted?
9. Does the lead accurately compress the body?
10. Would someone voluntarily read this article rather than only use it as a database record?

If the last answer is no, lint is not the next step. Reconsider the article object, evidence,
selection, and prose.
