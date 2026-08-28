# MediaWiki article structure

## Structure follows evidence

Use the smallest structure that makes the subject intelligible. Do not create empty or speculative sections to resemble a long encyclopedia article. Avoid generic “Impact,” “Legacy,” “Future,” “Broader context,” and “Conclusion” headings unless the evidence genuinely supports that specific treatment.

Write the lead last. It should define the subject, establish the most important supported facts, and summarize the article without citations doing hidden work. A relation to the host wiki or its community belongs in the lead only when it is independently important to identifying the subject and proportionate to the evidence.

## Templates and categories

Read the active site adapter and inspect live template data or examples before adding templates. Infobox fields are structured claims and require the same evidence as prose. Omit unknown parameters rather than guessing.

Categories describe established membership, not loose association. Verify that the category exists and that the subject belongs in it. Do not pad category counts.

## Citations

Use the adapter's citation families when available. Named references should have stable descriptive names. A reused reference must support every reuse. Do not populate archive fields unless an archive URL and date were actually inspected.

Every article that uses inline `<ref>` citations must also render them. Use the adapter's declared references template when one is available; otherwise add a `== References ==` section containing `<references />`. A clean claim map does not excuse an unrendered citation list.

Place citations close to the claims they support. Split citations when clauses have different evidence. Keep quotation attribution and citation in the same sentence or immediate context.

## Wikitext and extensions

Write wikitext, not Markdown. Use balanced templates, tags, tables, and links. Treat extension syntax as a target-wiki capability: inspect the active adapter and live capability manifest rather than assuming a standard MediaWiki installation provides it.

Prefer ordinary prose and simple wikitext over a complex template, module, chart, or gallery that does not materially improve comprehension. When a rich element is warranted, validate its inputs and inspect the rendered result.

## Mechanical closeout

Lint the exact file with the exact title. Apply only safe fixes, inspect their diff, and lint again. Verify live redirects and links when the local index may be stale. Mechanical validation confirms syntax and configured policy; it does not establish source fidelity, due weight, readability, or human acceptance.
