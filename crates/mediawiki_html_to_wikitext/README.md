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
Infobox interpretation is source-declared: the table class, title-row class, and observed field
layout are profile data rather than site-specific constants in the converter.
Admitted source message-box classes map only through a target-declared message template and
parameter vocabulary. Unlabeled infobox content uses an explicit target content parameter rather
than a fabricated label, and unlabeled audio derives its accessible label from captured source
filenames rather than generating article prose.

Generic webpage adaptation is semantic, not visual replication. A source profile may select one
meaningful content root, discard source-specific chrome or animation regions with explicit CSS
selectors, remove hidden duplicates, and drop embedded app elements. The result reports separate
counts for discarded style, script, interaction, hidden, and profile-selected structures. CSS is
therefore interpretation evidence for profile construction; it is not copied into the target wiki.
Wikitool never executes source JavaScript. When script execution is necessary to expose authored
content, an acquisition or browser layer must provide a bounded rendered DOM and its own provenance
before conversion. Durable text, media, captions, links, and informational states are then projected
through the target profile's MediaWiki primitives and admitted template vocabulary.

The crate does not acquire pages, decode a producer bundle, choose site templates, emit a
publication receipt, or write to a wiki. Those authorities belong to callers. Producer-specific
bundle decoding and Remilia's profile files stay outside the crate while the profile schemas and
conversion mechanics remain reusable and dogfoodable.
