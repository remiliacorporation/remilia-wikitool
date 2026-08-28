# MediaWiki HTML to wikitext

`mediawiki_html_to_wikitext` is the deterministic DOM-rendering layer used by preservation and
import companions. It converts retained HTML structure into conservative MediaWiki primitives,
routes same-origin links through a caller-supplied target, binds media only through a separately
verified inventory, and can map one explicitly admitted table shape to a configured template.

The crate does not acquire pages, decode a producer bundle, choose site templates, emit a
publication receipt, or write to a wiki. Those authorities belong to callers. This keeps source
adapters and Remilia-specific preservation policy out of the generic renderer while allowing the
same conversion mechanics to be dogfooded by the archive stack.
