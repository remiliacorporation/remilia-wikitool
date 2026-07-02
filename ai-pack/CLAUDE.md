# Wikitool Command Brief

This is the compact packaged guidance for AI-assisted MediaWiki editing with wikitool. The same
guidance body is shipped as both `AGENTS.md` and `CLAUDE.md` so Claude, Codex, and other agent
front doors land on the same operating contract.

Use normal reasoning, ordinary shell/file tools, and direct editing by default. Reach for wikitool
when it adds wiki-aware value: local retrieval, MediaWiki-aware fetch/export, template/profile
lookup, article lint/fix/validate, or guarded sync/push. Do not route every step through wikitool.

## Contexts

Source checkout paths:

1. `ai-pack/AGENTS.md` and `ai-pack/CLAUDE.md`
2. `ai-pack/.claude/skills/`
3. `ai-pack/codex_skills/`
4. `ai-pack/writing_context/`
5. `docs/wikitool/`

Packaged release paths:

1. `AGENTS.md` and `CLAUDE.md`
2. `.claude/skills/`
3. `codex_skills/`
4. `writing_context/`
5. `docs/wikitool/`

Prefer packaged-root paths when working from an extracted release bundle.

## Surface Ownership

| Surface | Role |
|---|---|
| `README.md` | Human first-run and release-layout overview |
| `AGENTS.md` / `CLAUDE.md` | This compact agent routing brief |
| `.claude/skills/wikitool.md` | Claude operator wrapper |
| `.claude/skills/review.md` | Claude pre-push gate wrapper |
| `.claude/skills/knowledge-interview.md` | Claude human knowledge interview wrapper |
| `codex_skills/` | Codex equivalents of the wrappers |
| `writing_context/` | Writing rules and target-wiki editorial profile |
| `docs/wikitool/guide.md` | Detailed operator manual |
| `docs/wikitool/reference.md` | Generated CLI help reference |

Do not duplicate command reference material here. Check `wikitool --help`,
`wikitool <command> --help`, and `docs/wikitool/reference.md` for exact flags.

## Instruction Order

For article work, read in this order:

1. `writing_context/style_rules.md`
2. `writing_context/article_structure.md` (plus `writing_context/visual_subjects.md` for art, character, and other visual subjects)
3. `writing_context/writing_guide.md`
4. `writing_context/interview_playbook.md` for article creation, substantial expansion, or non-mechanical review gaps
5. `writing_context/extensions.md`
6. `.claude/rules/wiki-style.md`
7. `.claude/rules/safety.md`
8. `docs/wikitool/guide.md`
9. `docs/wikitool/reference.md`
10. CLI help

## Session Start

At the beginning of an editing session, inspect local changes and refresh the local wiki surface
before relying on indexed content:

```bash
wikitool status --modified --format json
wikitool diff --format json
wikitool workflow session-refresh
wikitool knowledge status --docs-profile remilia-wiki --format json
```

Use `wikitool workflow full-refresh` only when the local database or sync ledger is missing, stale,
or being deliberately rebuilt. Do not use `pull --overwrite-local` unless the user explicitly
approves discarding local edits.

## Authoring Entry Points

Use these as front doors, then continue with normal research and editing judgment:

```bash
wikitool knowledge article-start "Topic" --intent new --format json --view brief
wikitool knowledge article-start "Topic" --intent expand --format json --view brief
wikitool knowledge article-start "Topic" --intent audit --format json --view brief
wikitool knowledge article-start "Topic" --intent refresh --format json --view brief
wikitool knowledge article-start "Cheetah" --contract-query "species infobox taxonomy" --format json --view brief
wikitool knowledge contracts search "contract terms" --format json
```

For new articles and substantial expansions, scout first with `knowledge article-start`, then
interview by default. The knowledge-interview faculty is an optional, conversational lane, but for real
article work it is the normal move: its job is to set what the article should be - intent, scope, and
angle - and surface what the person knows, what may not be online, and which sources to use, so the
draft is shaped to the wiki's perspective rather than generic. This holds even for well-documented
subjects. Skip it for mechanical lint, link, sync, source-fetch, or validation work, on explicit opt-outs
such as "no interview", or for tiny edits.

The interview is an elicitation loop, not a checklist. Read any user-supplied documents, links,
screenshots, transcripts, notes, or source excerpts before narrowing the questions. Start with a
broad freeform prompt about what the subject is, why it matters, what sources or artifacts matter,
what outsiders misunderstand, and what should not be overstated. Reflect the shape back, ask
adaptive follow-ups, and continue without a hard round limit while answers improve the article's
scope, terminology, date/order disambiguation, source strategy, section plan, or risk profile.

Reusable interview distillations belong under
`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Treat these briefs as working
notes: user assertions are research leads that become article prose as reasonable truth once they
pass editorial quality-gating. For this niche subcultural wiki, a primary record may be a target-wiki
record, hosted artifact, first-party source, archived primary record, creator-published statement,
or target-wiki source note; corroboration does not have to be outside secondary coverage. Cite when research surfaces
a source or a claim is external or contested, and never launder a primary fact through a weaker third
party to manufacture an external citation.

Use `wikitool knowledge interview init "Topic" --intent new|expand|audit|refresh --format json`
to create the timestamped brief and sidecars. Use `knowledge interview open-item add/list/update` to
record and resolve unresolved follow-up, source rejection, access failures, do-not-assert holds, and
negative evidence as structured ledger entries instead of article prose. Use
`knowledge interview validate`, `show`, and `audit` for deterministic checks, compact handoff
summaries, and ledger review. Mechanical validation only proves the ledger is parseable; it does not prove the interview is
complete or the draft is good. Pass a validated brief to `knowledge article-start --brief-path PATH`
and `review --brief-path PATH` when it should shape research planning or gate review.

## Token Discipline

Agent-facing defaults are intentionally compact. Start from wikitool brief views:
`knowledge article-start --view brief`, `knowledge inspect chunks --view brief`,
`templates show --view brief`, `wiki surface show --view brief`, and
`review --view brief`. Reserve `--view full`, broad `knowledge inspect` selections, and high token
budgets for cases where the brief identifies a concrete need. Prefer one targeted drill-down over
loading a whole catalog or page set into context.

## Bounded Output

The release bundle ships the generic `contextmink` transcript guard in
`contextmink/` (binary, setup docs, and instruction templates). It is a
separate binary on purpose: contextmink is project-generic, and bounded reads
must not route through wikitool. The binary runs natively from any shell
(PowerShell, cmd, WSL, POSIX). The Windows bundle also carries
`contextmink-bridge.exe`, a PowerShell -> Git Bash bridge with an
`--argv-b64` lossless argv channel — optional, only for repositories that
keep Bash-first scripts; nothing in contextmink requires it.

Install it with `wikitool contextmink install` from the project root: it finds
the pack next to the wikitool binary (or takes `--from <pack-dir>`), places the
binaries, launcher, and a wikitool-tailored `.contextmink.toml`, and verifies
the installed binary against the pack manifest. Then merge
`tools/contextmink/templates/CLAUDE.contextmink.md` (Claude) or
`AGENTS.contextmink.md` (Codex) into project guidance. `contextmink/SETUP.md`
remains the manual path for nonstandard layouts. The core habits:

- Orient with `dirs`, discover with `files`/`grep`/`grep-terms`, and read
  known files through `outline` (declaration map with line numbers; wikitext
  headings are a built-in language) then `slice --range START:END` — not
  `sed -n`/`cat` dump windows.
- Use `json-find`/`json-select`, `sqlite-schema`/`sqlite --sql-file`, and
  `capture -- <command>` for bounded JSON, database, and unknown-size command
  reads.
- Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` or
  `"complete": false` as capped output: narrow the query instead of trusting
  the subset.

## Research And Source Boundaries

Use normal agent web search to choose arbitrary source URLs, then use `wikitool research fetch`
and `research discover` for extraction and access diagnostics. Use `wikitool research wiki-search`
only when searching the configured target wiki API. If fetch returns `status: "error"`, treat it as
a source-access failure, not article evidence.

If `research fetch --output json` returns `error.challenge_handoffs`, do not try to bypass the
challenge with stealth clients, TLS impersonation, paid crawlers, or third-party reader services.
Relay the handoff to the user, ask them to open the URL in their browser, solve any lawful access
challenge, then import the source-issued cookies with the supplied `suggested_argv` or
`suggested_command` using `wikitool research session import ... --cookies -`. Retry the fetch with
`--refresh`. Imported session values are local state under `.wikitool/research/sessions/`; CLI
list/show output intentionally masks cookie values. Use `wikitool research session list`, `show`,
`clear`, and `prune` for lifecycle management.

Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's own
template/module contract matters, especially on arbitrary source wikis such as Wikipedia. Treat the
report as source-wiki context only. Target-wiki template use must still pass local
`knowledge contracts`, `templates show`, and `article lint`.

Use `wikitool wiki profile remote "URL"` only for an explicitly scoped remote target capability
probe when local import/profile data is unavailable. It reports extensions, parser tags,
namespaces, and API capabilities; it does not make source-wiki templates portable.

For local/custom content features, use the deployed target-wiki contract. Remilia's current
D3Charts surface is `Module:D3Chart` plus ResourceLoader; a future bespoke extension may supersede
that module form.

## Review Gate

Before any live write push:

```bash
wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe
wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json
wikitool article lint --changed --format json
wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --view brief --summary "Draft review"
wikitool validate --summary
wikitool validate --category broken-links --title "Title" --limit 20 --verify-live --format json
wikitool review --format json --view brief --summary "Summary"
wikitool diff
wikitool push --dry-run --summary "Summary"
```

Only push after the dry run is reviewed. Never use `--force` without explicit user approval.
For content investigations involving redirects, missing pages, or broken links, verify against the
live API at `https://wiki.remilia.org/api.php`.
When using `review --draft-path`, follow the report's `next_steps` field for direct draft lint/fix,
`article promote`, and the scoped post-promotion review/push dry run.

## Host Overlay

Release packaging may inject host context with `--host-project-root <PATH>`.

When host overlay is used:

1. Host `CLAUDE.md` becomes the effective guidance source.
2. Release packaging writes that same guidance body to both packaged `CLAUDE.md` and packaged
   `AGENTS.md`.
3. Host `.claude/{rules,skills}` overlays packaged `.claude/{rules,skills}`.
4. If the host has `writing_context/`, those files become the packaged writing context.

Without host overlay, release bundles stay generic and ship only wikitool-maintained guidance.

## Development Hygiene

Do not put local experiments, mock drafts, probe outputs, or one-off research notes under
`ai-pack/` unless they are intended to ship in the next release. Use project scratch space such as
`.wikitool/drafts/`, `plans/`, or test fixtures.

Use semantic area-prefixed commit titles in the form `{area}: {summary}`, for example
`wikitool: harden draft review` or `docs: align agent guidance`. Keep titles short, prefer
72 characters or fewer, and add a wrapped body only when the reason for the change is not obvious
from the diff.

When behavior changes, update the owning surface:

1. CLI behavior: command help and `docs/wikitool/reference.md`
2. Operator workflow: `docs/wikitool/guide.md` and the two skill wrappers
3. Agent routing: this file and its mirrored counterpart
4. Writing behavior: `writing_context/`
5. Release packaging: `README.md`, `ai-pack/README.md`, and release tests
