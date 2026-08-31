# HTML-to-wikitext conversion

Wikitool's public source workspace includes `mediawiki_html_to_wikitext`, a library for bounded,
deterministic conversion of retained HTML5 DOM structure to MediaWiki wikitext. It exists so import
and preservation applications do not each invent their own escaping, whitespace, link, list,
table, blockquote, preformatted-text, and media-occurrence behavior.

The library's boundary is intentionally narrower than an importer. Its profiled entry point takes:

- the exact HTML fragment and canonical source URL;
- a strict capture receipt binding those exact bytes and their acquisition representation;
- a source profile for source identity, article/media routes, and observed HTML semantics;
- a target profile for link routing, output bounds, and the admitted template vocabulary;
- an opaque media scope;
- a separately verified media inventory, either URL-keyed or ordered by DOM occurrence.

The default binary exposes that reusable boundary without adding a producer adapter:

```bash
wikitool import html-to-wikitext evidence/page.html \
  --source-profile profiles/source.json \
  --target-profile profiles/target.json \
  --canonical-title "Example" \
  --canonical-url "https://source.example/wiki/Example" \
  --source-key source-example \
  --capture-receipt evidence/capture-receipt.json \
  --media-scope source-example \
  --media-inventory evidence/media-inventory.json \
  --output .wikitool/imports/Example.wiki \
  --format json
```

The output must remain under the selected Wikitool project's scoped content, template, or
`.wikitool` directories. Source profiles explicitly declare any admitted infobox table class,
title-row class, and observed field layout; the converter does not carry a hidden source-site
default. The target profile alone selects destination templates and their parameter vocabulary.

The required `mediawiki.html-capture-receipt.v1` records the source key, canonical and final URLs,
capture time, static-HTML or rendered-DOM representation, producer name and version,
JavaScript-execution claim, timeout, exact HTML hash and byte count, and bounded resource-observation
limits. These fields make byte and representation drift observable, but remain caller-reported
provenance rather than authenticated proof of the browser or acquisition environment.

The converter returns exact hashes for the receipt, HTML, profiles, optional media inventory, and
generated wikitext alongside structural coverage, used media identities, the number of ordered
occurrences consumed, and typed `unmapped_structures`. It also reports bounded typed observations
for inline styles, external stylesheets, inline scripts, and external scripts. Inline content is
represented only by a hash and byte count; safe external resources retain declared and resolved
locators, media or script type, a narrow classification, and a disposition showing that CSS was not
applied or JavaScript was not executed by the converter. Invalid, unsupported-scheme,
credential-bearing, or overlong locators are rejected or reduced to a hash and typed status rather
than echoed. Hard URL, observation, and aggregate inline-byte ceilings prevent a small receipt from
amplifying into an unbounded report. The converter never fetches external resource bodies, applies a
CSS cascade, or executes JavaScript.

An unfamiliar source table is evidence for template review—not authority to copy the source site's
template vocabulary. Conversion fails on unsupported retained structured elements, unsafe targets,
missing media bindings, occurrence drift, invalid media types, receipt drift, and exceeded evidence
bounds. It does not fetch a page, decide which content is editorially valuable, infer unconfigured
template semantics, decode a producer schema, generate an acceptance decision, or publish anything.

Applications own the surrounding evidence contract. A target-specific archive companion, for
example, can validate private bundle schemas through a producer adapter, supply tracked source and
target profiles, admit a live Wikitool authoring surface, and emit an exact archive transform
receipt. Producer schemas and archive commands are deliberately absent from the default `wikitool`
executable; only the normalized profiled compiler is public there.
