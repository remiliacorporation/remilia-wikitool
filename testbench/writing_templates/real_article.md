# Real Article Test Template

Research and write a complete wiki article for a real topic.

## Topic
- **Name**: {{TOPIC_NAME}}
- **Domain**: {{TOPIC_DOMAIN}}
- **Context**: {{TOPIC_CONTEXT}}

## Constraint
{{CONSTRAINT_RULE}}

## Style Trap
{{STYLE_TRAP_CHECK}}

## Instructions

1. Research the topic using web search and wiki API
2. Write raw MediaWiki wikitext (NOT markdown)
3. Follow all wiki style rules from `custom/wikitool/ai-pack/llm_instructions/style_rules.md`
4. Obey the constraint above exactly
5. Use only verifiable claims with real citations
6. The style trap describes what the evaluator will check — write to pass it

## Required Structure

```
Line 1: {{SHORTDESC:Brief description}}
Line 2: {{Article quality|unverified}}
Line 3: (blank)
Line 4+: '''Topic Name''' followed by opening paragraph
...body sections...
== References ==
{{Reflist}}

[[Category:...]]  (2-4, looked up from wiki database)
```

## Output

Save the article to `wiki_content_testing/Main/{{TOPIC_NAME_ENCODED}}.wiki`

Good real-topic articles may be manually promoted to `wiki_content/Main/` after review.
