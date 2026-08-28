# MediaWiki HTML to wikitext

`mediawiki_html_to_wikitext` is the deterministic DOM-rendering layer used by preservation and
import companions. It converts retained HTML structure into conservative MediaWiki primitives,
routes same-origin links through a caller-supplied target, binds media only through a separately
verified inventory, and composes independently versioned source and target profiles.

A source profile owns source identity, article/media routes, ordinary table classes, and observed
source-side semantics. A target profile owns link routing, output bounds, and the admitted template
vocabulary. Only the profiled compiler may join a source semantic such as an infobox-shaped table
to a target template. Other classified tables are returned as typed `unmapped_structures`, so
corpus expansion informs deliberate target-template design instead of cloning source templates.

The crate does not acquire pages, decode a producer bundle, choose site templates, emit a
publication receipt, or write to a wiki. Those authorities belong to callers. Producer-specific
bundle decoding and Remilia's profile files stay outside the crate while the profile schemas and
conversion mechanics remain reusable and dogfoodable.
