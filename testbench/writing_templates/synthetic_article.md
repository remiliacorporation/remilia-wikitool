# Synthetic Article Test Template

Write a complete wiki article for the fictional topic below.

## Topic
- **Name**: {{TOPIC_NAME}}
- **Domain**: {{TOPIC_DOMAIN}}
- **Context**: {{TOPIC_CONTEXT}}

## Constraint
{{CONSTRAINT_RULE}}

## Style Trap
{{STYLE_TRAP_CHECK}}

## Instructions

1. Write raw MediaWiki wikitext (NOT markdown)
2. The topic is entirely fictional — invent plausible details
3. Follow all wiki style rules from `custom/wikitool/ai-pack/llm_instructions/style_rules.md`
4. Obey the constraint above exactly
5. The style trap describes what the evaluator will check — write to pass it

## Required Structure

```
Line 1: {{SHORTDESC:Brief description}}
Line 2: {{Article quality|unverified}}
Line 3: (blank)
Line 4+: '''Topic Name''' followed by opening paragraph
...body sections...
== References ==
{{Reflist}}

[[Category:...]]
```

## Output

Save the article to `wiki_content_testing/Main/{{TOPIC_NAME_ENCODED}}.wiki`
