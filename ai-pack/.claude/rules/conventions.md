# Team Conventions (AI Pack)

These conventions are always active for Remilia Wiki editing work.

## Canonical references

Use this lookup order:

1. `CLAUDE.md` (same content as `AGENTS.md`)
2. `llm_instructions/style_rules.md`
3. `llm_instructions/article_structure.md`
4. `llm_instructions/writing_guide.md`
5. `docs/wikitool/how-to.md`
6. `docs/wikitool/reference.md`
7. `wikitool --help` and `wikitool <command> --help`

## Path context mapping

Packaged release artifacts use bundle-root paths:

1. `llm_instructions/*`
2. `docs/wikitool/*`
3. `.claude/rules/*`
4. `.claude/skills/*`

Source repository layout uses:

1. `ai-pack/llm_instructions/*`
2. `docs/wikitool/*`
3. `ai-pack/.claude/rules/*`
4. `ai-pack/.claude/skills/*`

## Editing scope

This pack targets content/editorial work:

1. Article and template editing
2. Research and source verification
3. Cleanup/review and page-level SEO diagnostics

Out of scope by default:

1. Infrastructure/deployment/server operations
2. Release engineering workflows
3. Risky write operations without explicit user direction

## Working directory

Run `wikitool` commands from the active project root (the folder that contains `.wikitool/` and `wiki_content/`).
