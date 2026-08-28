# HTML-to-wikitext conversion

Wikitool's public source workspace includes `mediawiki_html_to_wikitext`, a library for bounded,
deterministic conversion of retained HTML5 DOM structure to MediaWiki wikitext. It exists so import
and preservation applications do not each invent their own escaping, whitespace, link, list,
table, blockquote, preformatted-text, and media-occurrence behavior.

The library's boundary is intentionally narrower than an importer. Its profiled entry point takes:

- the exact HTML fragment and canonical source URL;
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
  --media-scope source-example \
  --media-inventory evidence/media-inventory.json \
  --output .wikitool/imports/Example.wiki \
  --format json
```

The output must remain under the selected Wikitool project's scoped content, template, or
`.wikitool` directories. Source profiles explicitly declare any admitted infobox table class,
title-row class, and observed field layout; the converter does not carry a hidden source-site
default. The target profile alone selects destination templates and their parameter vocabulary.

The converter returns exact hashes for the HTML, profiles, optional media inventory, and generated
wikitext alongside structural coverage, used media identities, the number of ordered occurrences
consumed, and typed `unmapped_structures`. An unfamiliar source table is evidence for template
review—not authority to copy the source site's template vocabulary. It fails on
unsupported retained structured elements, unsafe targets, missing media bindings, occurrence
drift, and invalid media types. It does not fetch a page, decide which content is editorially
valuable, infer unconfigured template semantics, decode a producer schema, generate an acceptance
decision, or publish anything.

Applications own the surrounding evidence contract. The Remilia PreservationArchive companion,
for example, validates private Minibeast bundle schemas through a producer adapter, supplies the
tracked TCRF source profile and Remilia target profile, admits the live Wikitool authoring surface,
and emits the exact archive transform receipt. The producer schemas and archive commands are
deliberately absent from the default `wikitool` executable; only the normalized profiled compiler
is public there.
