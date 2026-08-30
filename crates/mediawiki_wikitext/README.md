# MediaWiki source wikitext

`mediawiki_wikitext` is a deterministic structural parsing and rewriting boundary for exact
MediaWiki source text. It exists for preservation and import companions that already possess a
revision-bound source document and need to interpret its templates, parameters, links, redirects,
and literal regions without executing source-wiki code.

The public boundary has two independently versioned inputs:

- `mediawiki.wikitext-source-profile.v1` supplies source identity, byte and nesting bounds,
  source-reported file/category namespace aliases, and redirect magic words.
- `mediawiki.wikitext-source-document.v1` supplies page and revision identity plus the exact UTF-8
  byte length and SHA-256 digest. `parse_source_document` verifies these fields before parsing.

The parser is a character/state machine, not a regular-expression recognizer. It returns preorder,
span-addressed nodes for balanced `{{templates}}`, `{{{parameters}}}`, and `[[internal links]]`.
Template arguments retain named versus positional identity; file links retain every option/caption
component; links are classified as main, file, category, other-namespace/interwiki, or fragment
using the caller's profile, and page-qualified fragments are exposed separately. HTML comments and
`nowiki`, `pre`, `source`, and `syntaxhighlight`
regions are retained as protected nodes and their contents are never parsed as wikitext.
Closing markers without a corresponding open construct remain literal source text; an open
construct that never closes is an explicit parse error.
A `[[` candidate whose unexpanded title contains a raw `[` or lone `]` is likewise literal rather
than a link, while a syntactically possible `[[Title` remains an unclosed-link error.

Rewriting is deliberately explicit. A caller builds a `RewritePlan` from parsed `NodeId` values and
provides the complete replacement for each selected node. The document rejects overlapping
ancestor/descendant replacements rather than choosing an implicit winner. An empty plan is an exact
round trip.

This is not a complete MediaWiki parser or template engine. It does not acquire pages, enumerate a
corpus, decode a producer-specific manifest, expand templates or parser functions, apply site
configuration, choose target-wiki semantics, emit a transformation receipt, or publish content.
Those authorities belong to callers. Private acquisition tools can translate their page records
into the neutral document receipt without becoming Wikitool runtime dependencies; site-specific
template mappings and digest-bound transformation receipts likewise remain in the consuming
projector.
