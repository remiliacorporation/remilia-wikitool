# Style Trap Detection Criteria

## Pass Criteria (all articles)

| # | Check | Pass Condition |
|---|-------|----------------|
| 1 | Line 1 | Starts with `{{SHORTDESC:` |
| 2 | Line 2 | Equals `{{Article quality|unverified}}` |
| 3 | Line 3 | Is blank |
| 4 | Line 4+ | Starts with `'''` (bold title) |
| 5 | References | Contains `== References ==` with `{{Reflist}}` |
| 6 | Categories | Has 2-4 `[[Category:...]]` lines |
| 7 | Banned phrases | Zero matches from style_rules.md banned list |
| 8 | AI watchlist | Fewer than 3 AI watchlist words |
| 9 | Quotes | Straight quotes only (`"` and `'`), no curly quotes |
| 10 | Format | No markdown syntax (`#`, `**`, `` ` ``, `---`) |
| 11 | Constraint | Constraint-specific rule from writing_pools.json |

## AI Watchlist Words

These words individually are not errors, but 3+ in one article suggests AI-generated style:

- delve, tapestry, multifaceted, landscape, realm
- paradigm, robust, leverage, foster, comprehensive
- innovative, utilize, facilitate, synergy, holistic

## Banned Phrases (significance inflation)

- groundbreaking, revolutionary, pivotal, transformative
- game-changing, cutting-edge, state-of-the-art
- "one of the most important", "widely regarded as"
- "played a key role", "had a profound impact"

## Banned Sources

- IQ.wiki
- Know Your Meme
- NFT Price Floor
- Urban Dictionary

## Structural Anti-Patterns

- Bullet lists where prose is expected (unless specifically a list section)
- `== External links ==` with only one link
- Categories that don't exist in the wiki
- Wikilinks to obviously nonexistent articles
- Citation stubs (url only, no title/date)
- Empty sections (heading followed immediately by another heading)
