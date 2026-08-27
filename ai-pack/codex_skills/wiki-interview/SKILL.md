---
name: wiki-interview
description: Interview a human to capture article scope, firsthand knowledge, source leads, exclusions, terminology, chronology, and unresolved claims in a neutral Wikitool ledger. Use before research or drafting when important knowledge is not already recorded.
---

# Wiki interview

Conduct an adaptive editorial interview while preserving the human's knowledge and boundaries in a neutral, machine-validated ledger. The skill chooses questions and follow-ups; Wikitool records paths, scout facts, metadata, open items, and freshness.

## Required context

1. Read [interview-ledger.md](references/interview-ledger.md).
2. Read every material the human supplied before asking narrowing questions.
3. Run `wikitool wiki profile show --format json` and read project-owned supplemental guidance when the subject or site has specialized terminology, privacy, or source rules.
4. Use current CLI help for command flags.

## Procedure

### 1. Establish purpose and consent

Confirm the proposed title or topic, intent (`new`, `expand`, `audit`, or `refresh`), and what the human wants the eventual article to accomplish. Make clear that the interview is a knowledge and source-intake step, not automatic publication.

Ask whether any supplied material or part of the conversation is private, off the record, attribution-sensitive, uncertain, or not for article prose. Record those boundaries exactly.

### 2. Inspect materials and local state

Read supplied documents, notes, transcripts, drafts, screenshots, and links first. Then initialize the ledger with the local scout unless the user explicitly wants a blank record:

```text
wikitool knowledge interview init "TITLE" --intent new --format json
```

Inspect the generated brief and scout facts. Local existence, comparable pages, templates, categories, and missing query terms should sharpen questions, not dictate the article's framing.

### 3. Invite an open account

Begin with one broad invitation suited to the subject, such as asking what it is, why it matters, how it developed, what people misunderstand, what artifacts or sources to inspect, and what would be disappointing to omit.

Let the human finish a coherent account before imposing a section scheme. Reflect back the article object and the most important distinctions in plain language, then ask for corrections.

### 4. Follow evidence-shaped gaps

Ask one focused follow-up at a time. Choose questions from what is missing or contradictory in the actual materials and scout, including:

- identity and boundaries of the subject;
- chronology, versions, names, ownership, or roles;
- firsthand versus secondhand knowledge;
- exact source leads and where within them to look;
- terminology that outsiders may misuse;
- relationships that are central, peripheral, contested, or merely local context;
- claims that require independent verification;
- privacy, safety, legal, or reputational holds;
- what the existing article gets wrong or omits, when revising.

Do not read a canned questionnaire. Do not force every category to have an answer. Do not ask the human to pre-write polished encyclopedia prose.

### 5. Maintain the neutral ledger

Update the generated brief throughout the interview. Keep separate:

- supplied materials and provenance;
- the human's account and terminology;
- source leads;
- unresolved or disputed claims;
- explicit exclusions and holds;
- optional human suggestions about article shape.

Use structured open items for missing sources, rejected sources, disputed links, privacy exclusions, do-not-assert items, and follow-up needs. Do not resolve an item merely because the human repeats the claim.

### 6. Critically reflect, then return to the human

Before closing, privately test whether the record would lead an author toward a thin, duplicative, wrongly scoped, or misleading article. Convert that diagnosis into concrete follow-up questions, not conclusions inserted into the ledger as tool policy.

Ask the human to confirm the article object, key chronology, terminology, material source leads, and exclusions. Preserve disagreement rather than forcing consensus.

### 7. Validate and hand off

Run:

```text
wikitool knowledge interview validate PATH --format json
```

Resolve structural errors and explain meaningful warnings. Validation proves that the ledger is parseable, current, and internally linked; it does not make human statements independent evidence and does not make the article drafting-ready by itself.

Hand off the brief path, source packet, unresolved open items, and a short account of what requires research. Invoke `wiki-writing` only when authoring is requested.

## Exit conditions

Finish only when:

- the human has confirmed the article object and scope;
- supplied materials were inspected before narrowing;
- firsthand knowledge is distinguishable from verified evidence;
- source leads are concrete enough to inspect;
- exclusions, privacy limits, and do-not-assert items are recorded;
- contradictions and blocking gaps remain visible;
- the ledger validates or its structural blocker is reported;
- no prose acceptance or publication is implied.
