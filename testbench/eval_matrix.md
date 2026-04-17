# Wikitool Eval Matrix

This file captures the usefulness eval surface for the post-cutover workflow.

Use it with:

1. `testbench/cli_tests.sh` for broad regression coverage
2. `testbench/acceptance_workflows.sh` for focused workflow acceptance
3. `writing_pools.json` plus `writing_templates/` for human or agent-scored authoring evals

## Workflow evals

### 1. Greenfield article, no local page

- Input: topic from `writing_pools.json` that does not exist locally
- Commands:
  - `wikitool knowledge article-start "<Topic>" --format json`
  - optional `wikitool research wiki-search "<Topic>" --format json`
  - optional `wikitool research fetch "<URL>" --output json`
- Review:
  - Does `article-start` propose a plausible article type?
  - Is the section skeleton useful for a first draft?
  - Are the open questions concrete and non-generic?

### 2. Weakly linked local mention only

- Fixture shape: one local page links to the target, but the target page does not exist
- Commands:
  - `wikitool knowledge build`
  - `wikitool knowledge article-start "<Missing Topic>" --format json`
- Review:
  - `local_state` should surface `linked_but_missing`
  - comparable pages and link/category suggestions should reflect the local graph

### 3. Existing page refactor

- Fixture shape: existing article with weak structure or outdated conventions
- Commands:
  - `wikitool knowledge article-start "<Existing Topic>" --format json`
  - `wikitool article lint wiki_content/Main/<Existing_Topic>.wiki --format json`
- Review:
  - Does `article-start` still give useful refactor guidance rather than only greenfield hints?
  - Does lint point to concrete remediation instead of generic style nagging?

### 4. Template-heavy article

- Fixture shape: article depending on infobox and citation-family choices
- Commands:
  - `wikitool templates catalog build --format json`
  - `wikitool templates show "Template:..." --format json`
  - `wikitool knowledge article-start "<Topic>" --format json`
- Review:
  - Are recommended templates aligned with local template usage and examples?
  - Are template examples sufficient for an agent to fill parameters correctly?

### 5. Citation-heavy concept article

- Fixture shape: concept page requiring multiple citations and appendix sections
- Commands:
  - `wikitool research wiki-search "<Topic>" --format json`
  - `wikitool research fetch "<URL>" --output json`
  - `wikitool article lint wiki_content/Main/<Topic>.wiki --format json`
- Review:
  - Does research extraction preserve enough metadata to build good citations?
  - Does lint catch citation placement and missing reflist issues without overfiring?

### 6. Remilia-specific page requiring profile rules

- Fixture shape: article using Remilia-specific terminology and infobox conventions
- Commands:
  - `wikitool wiki profile sync --format json`
  - `wikitool wiki rules show --format json`
  - `wikitool article lint wiki_content/Main/<Topic>.wiki --format json`
- Review:
  - Are profile rules reflected in lint and template guidance?
  - Does the tool prefer `parent_group` and the correct references template?

## Quality eval rubric

Score each axis `0`, `1`, or `2`.

- `0`: wrong or unusable
- `1`: partially useful, needs substantial cleanup
- `2`: directly useful with only light cleanup

Axes:

- Infobox recommendation accuracy
- Category recommendation quality
- Citation family recommendation quality
- Section skeleton usefulness
- Lint precision / false-positive rate
- Research extraction quality
- Docs-bridge usefulness

Record notes beside each score with the specific command output that justified it.

## Latency / cost evals

Record wall-clock time and whether the run hit local-only or network-backed surfaces.

Commands:

- `wikitool knowledge article-start "<Topic>" --format json`
- `wikitool knowledge article-start "<Topic>" --format json --include-pack`
- `wikitool research fetch "<URL>" --output json`
- `wikitool article lint wiki_content/Main/<Topic>.wiki --format json`
- `wikitool wiki capabilities sync --format json`

Track:

- command
- topic or URL
- runtime tier (`offline` or `live`)
- elapsed seconds
- cache status when applicable
- subjective usefulness note

## Release gate mapping

Phase 6 is considered healthy when:

1. `cli_tests.sh` is green
2. `acceptance_workflows.sh` is green on the intended tier
3. README / guide / how-to / explanation / AI-pack all point to the same default workflow
4. low-level commands remain documented as advanced or raw primitives
