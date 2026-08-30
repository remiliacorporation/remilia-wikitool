# Exact source-wikitext parsing

Wikitool's public source workspace includes `mediawiki_wikitext`, a small library for consuming
revision-bound MediaWiki source text. It complements `mediawiki_html_to_wikitext`: exact wikitext is
the preferred semantic input when a source wiki exposes it, while retained HTML remains a distinct
fallback for sources that do not.

The source-wikitext boundary accepts:

- a strict `mediawiki.wikitext-source-profile.v1` profile containing a source key, maximum byte and
  nesting bounds, source-reported file/category namespace aliases, and redirect magic words;
- a strict `mediawiki.wikitext-source-document.v1` receipt containing page, namespace, revision,
  timestamp, and content-model identity plus exact UTF-8 byte length and SHA-256 digest;
- the source bytes supplied by the caller.

The parser verifies the receipt before it exposes preorder, byte-span-addressed syntax nodes. It
balances template invocations, triple-brace parameters, and internal links; separates named and
positional template arguments; retains all file-link components; classifies links through the
source profile; exposes page-qualified fragments; and identifies leading redirects. Comments and
literal `nowiki`, `pre`, `source`, and `syntaxhighlight` regions are opaque protected nodes. Parsing
is bounded and fails locally on unclosed open constructs, malformed protected tags, or over-depth
nesting. Closing markers without an open construct remain literal source text, matching ordinary
code and prose behavior. Link recognition also leaves raw bracket expressions literal when their
would-be unexpanded title contains `[` or a lone `]`; syntactically possible unclosed links still
fail locally.

A `RewritePlan` names parsed nodes and their complete replacements. Replacements must not overlap,
which forces a consuming projector to choose whether an outer template or one of its descendants
owns a rewrite. With no replacements, output is byte-identical to the supplied source. The library
does not expand source templates or silently assign target semantics.

Corpus producers remain adapters. They enumerate and refresh pages, store bytes and media, and
produce their own manifests. A caller translates one verified page record into the neutral public
document receipt; Wikitool does not import or run the producer. A target-specific projector owns
template mappings, link/media routing, target capability checks, and a digest-bound transformation
receipt joining producer evidence, source profile, mapping profile, and output.
