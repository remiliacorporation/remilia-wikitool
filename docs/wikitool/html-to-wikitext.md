# HTML-to-wikitext conversion

Wikitool's public source workspace includes `mediawiki_html_to_wikitext`, a library for bounded,
deterministic conversion of retained HTML5 DOM structure to MediaWiki wikitext. It exists so import
and preservation applications do not each invent their own escaping, whitespace, link, list,
table, blockquote, preformatted-text, and media-occurrence behavior.

The library's boundary is intentionally narrower than an importer. A caller supplies:

- the exact HTML fragment and canonical source URL;
- a route and fragment policy for same-origin links;
- an opaque media scope plus configured image/audio templates;
- an optional, explicitly admitted table-to-template mapping;
- a separately verified media inventory, either URL-keyed or ordered by DOM occurrence.

The converter returns wikitext, structural coverage, used media identities, and the number of
ordered occurrences consumed. It fails on unsupported retained structured elements, unsafe
targets, missing media bindings, occurrence drift, and invalid media types. It does not fetch a
page, decide which content is editorially valuable, infer template semantics, decode a producer
schema, generate an acceptance decision, or publish anything.

Applications own the surrounding evidence contract. The Remilia PreservationArchive companion,
for example, lives beside `archive-compiler`: it validates Minibeast bundles, maps its source
inventory into the generic converter types, admits the live Wikitool authoring surface, and emits
the exact archive transform receipt. Those schemas and commands are deliberately absent from the
default `wikitool` executable.
