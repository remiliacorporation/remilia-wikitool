# Wiki Writing Style Rules (AI Pack)

Use this compact rule set in every wiki-content task.

## Non-negotiable references

1. `llm_instructions/style_rules.md`
2. `llm_instructions/article_structure.md`
3. `llm_instructions/writing_guide.md`

## Output requirements

1. Return raw MediaWiki wikitext, not Markdown.
2. Use straight quotes (`"` and `'`), never curly quotes.
3. Keep prose neutral, factual, and encyclopedic.

## Structure requirements

Every new AI-drafted article must include:

1. `{{SHORTDESC:...}}` on line 1
2. `{{Article quality|unverified}}` on line 2
3. references and valid categories

## Citation requirements

1. Use reliable primary/official sources for factual claims.
2. Use named refs for repeated citations.
3. Strip URL tracking params.
4. Keep archive fields blank for later human completion.
