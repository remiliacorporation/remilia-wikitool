# Wikitool authoring eval matrix

This matrix tests whether wikitool helps an agent produce evidence-bound encyclopedic prose that a
human would choose to read. It does not reward apparent completeness, fixed length, vocabulary
avoidance, or the number of headings and categories.

Use it with:

1. `testbench/cli_tests.sh` for deterministic CLI regression coverage;
2. `testbench/acceptance_workflows.sh` for workflow acceptance;
3. `writing_pools.json` and `writing_templates/` for controlled and real-corpus authoring evals;
4. at least one named human reader for publication-quality judgments.

## Hard failures

An eval is failed regardless of the score when the draft:

- invents a fact, quotation, date, source, citation field, or relationship;
- uses model memory or neighboring-page text as evidence;
- attaches a source to a claim it does not support;
- publishes a `do_not_assert`, rejected, or negative-evidence item as fact;
- gives unsupported contentious material about an identifiable person;
- defines the subject primarily through a low-weight Remilia or Charlotte Fang connection;
- hides a contradiction or missing source behind confident synthesis;
- contains article-shaped filler that exists only to meet an expected length or structure.

## Controlled-evidence evals

Use `writing_templates/synthetic_article.md` with a case from `writing_pools.json`. These are
closed-world fixtures: the packet is the entire factual universe for the test. The agent must write
no fact beyond it. Fixture URLs use `.invalid` deliberately and outputs must remain under
`wiki_content_testing/`; they are never promotion candidates.

Evaluate:

1. **Relationship trap** — a minor Remilia mention must not take over the lead or create a default
   relationship section.
2. **Sparse evidence** — the agent should produce a short, exact article rather than padding it.
3. **Living-person risk** — rejected gossip and unsupported motive must be omitted.
4. **Contradictory chronology** — the agent should distinguish announcement, first public evidence,
   and unresolved conflict rather than choosing a date silently.

## Real-corpus evals

Use `writing_templates/real_article.md` for current pages and source-backed topics. Minimum corpus:

- `Clavicular` — direct subject definition, gratuitous relationship framing, repeated “most known”
  language, sensitive claim/source binding, and citation reuse;
- one strong existing page — preservation test; the agent should not rewrite merely to homogenize;
- one visual work — artifact description versus influence/intent interpretation;
- one thin or missing subject — honest short-form authoring or redirect recommendation;
- one actual Remilia project — correct use of Remilia framing and `parent_group` when supported.

For each real page, retain before/after rendered text, the inspected source map, lint output, and the
human editor's decision.

## Paired steering eval

For the same topic and source packet, produce two blinded drafts:

- **A:** writing context plus the sources, without `knowledge article-start` output;
- **B:** the same inputs plus `knowledge article-start --view brief`.

Randomize presentation to human reviewers. B should improve target integration, gaps, and
terminology without adding unsupported facts, copied structure, category frequency bias, or
host-wiki relationship framing. If B is worse, the retrieval/steering layer is a regression even
when its output is internally correct.

## Existing-page audit eval

Run `knowledge article-start --intent audit`, lint, and a source review. The agent must distinguish:

- factual/source defects;
- prose and proportionality defects;
- target-wiki mechanics;
- good prose to preserve;
- missing evidence that prevents a responsible rewrite.

Score the diagnosis separately from the rewritten article. Finding a defect does not prove the
proposed fix is better.

## Quality rubric

Score each axis from `0` to `4` after checking hard failures:

- **0 — unusable:** wrong, fabricated, or structurally misleading;
- **1 — poor:** major repair needed;
- **2 — serviceable:** factual core exists but substantial editing remains;
- **3 — good:** useful and trustworthy with bounded edits;
- **4 — excellent:** publishable after ordinary copy review.

Axes:

1. source fidelity and citation-to-claim binding;
2. direct subject definition and stable scope;
3. specificity and factual density;
4. proportionality and editorial selection;
5. structure derived from reader needs and evidence;
6. neutral, readable paragraph craft;
7. uncertainty, contradiction, and attribution handling;
8. living-person and sensitive-claim care;
9. target-wiki integration without profile-default leakage;
10. preservation of good existing material;
11. human reader value: “Would I voluntarily read this to understand the subject?”

Record a short evidence note beside every score. Automated phrase counts cannot substitute for any
axis.

## Deterministic workflow evals

Verify that:

- `article_start_v3` exposes `agent_may_draft_prose: true`,
  `model_output_is_evidence: false`, and the human publication gate;
- exact-subject pages are excluded from comparables;
- evidence references retain stable source paths and content hashes;
- `drafting_ready` requires current, clean content and docs artifacts;
- lint flags configured synthetic phrasing and forced relationship headings without rewriting them;
- `article accept` records the truthful origin and exact hash;
- changed Main prose cannot promote or push with a missing/stale receipt, including with `--force`;
- Contextmink setup is owned and receipted by Contextmink itself.

## Release health

The authoring surface is healthy only when deterministic tests pass, controlled packets have no hard
failures, real-corpus audits improve or preserve human-rated quality, and paired steering shows that
retrieved wiki context helps more often than it distorts.
