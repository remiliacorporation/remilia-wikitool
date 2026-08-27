# Prose-review manual contract audit

Date: 2026-08-27

This artifact records a same-agent manual application of the shipped `prose-review` procedure to
the closed-world fixtures. The expectation file was visible in the same evaluator context before
the findings were frozen. This is therefore a contract adjudication, not a blinded forward eval,
an independence claim, or evidence of model-version quality.

Inputs:

- `ai-pack/codex_skills/prose-review/SKILL.md` — SHA-256
  `560097c3b15f5fbf04be53f960da3517ae1c4a08be79e5eab9d74cfdfdf6df37`
- `testbench/prose_review_cases.json` — SHA-256
  `bc8c1ee8bc3e09f323b6ed439ca0a2e7b9ba1e761e37746167f556354e56b8f3`
- `testbench/prose_review_expectations.json` — SHA-256
  `d965ae79ab39c30dd380802e7db6756b8032326d38de5a22556b56cf8d3c07c8`

## `sensitive_claim_without_support`

- **P1 — block, lead:** The cited studio profile contains no support for the arrest, cocaine, or
  rehabilitation claims. These are direct living-person, drug, crime, and health assertions, so
  human acceptance or clean lint cannot cure the evidence failure. Remove the claims from prose
  and retain them as held items unless direct high-quality sources establish that they are both
  accurate and proportionate.
- The `Work` paragraph is supported by the exhibition checklist and is not implicated by the
  blocker.
- **Reader verdict:** No. Unsupported biographical sensationalism displaces the documented work.
- **Source verdict:** Complete for this closed-world packet; the sensitive claims are unsupported.
- **Disposition:** `block`.

The required blocking distinction is present and neither forbidden false positive is introduced.

## `gratuitous_host_relationship`

- **P2 — revise, short description and lead:** The activity log supports only that someone shared
  one link. It does not establish affiliation, collaboration, an “orbit,” or broader cultural
  significance. The article makes the true but minor event its definition while withholding the
  useful archive identity until the body. Lead with the volunteer archive, its 2022 establishment,
  its 480-poster scope, and its organization. Remove the host relationship or move a precisely
  attributed version later only if it earns relevance in the final article.
- The archive facts are directly supported and should be preserved.
- **Reader verdict:** Not in its current order. The lead withholds the useful definition in favor
  of local relevance.
- **Source verdict:** Complete for the closed-world claims.
- **Disposition:** `revise`.

The review does not call the shared link false and does not recommend deleting the archive facts.

## `source_specific_clean_article`

No blocking source-fidelity, weight, or sensitive-person finding. The article conservatively
distinguishes three propositions: the group was announced as launching on 14 March, the earliest
preserved session page is dated 9 May, and the available record does not establish the first
meeting date. It does not infer either that a meeting happened on 14 March or that no March or April
meeting occurred.

- **Reader verdict:** Yes, as a concise record. It explains the evidentiary date distinction
  without padding.
- **Source verdict:** Complete for this closed-world packet.
- **Disposition:** `accept`.

No invented significance, reception, or chronology defect is introduced.

## Result and limitation

The written procedure and fixture contract are aligned: the two defects are distinguishable from
the clean control, and the prescribed review format does not require cosmetic findings. Before a
release claims prose-review quality, run the same cases with expectations physically withheld from
the reviewer, record the model and harness version, and obtain independent adjudication. Add real
source packets and strong existing articles so an evaluator that reflexively rewrites or blocks
cannot pass.
