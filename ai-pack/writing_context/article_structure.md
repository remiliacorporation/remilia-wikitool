# Article structure

Structure is an editorial result, not a template to fill. Start from the article's supported factual
spine, cluster related claims, and choose the smallest set of headings that helps a reader follow
them. `knowledge article-start` reports observed section candidates and comparable outlines; these
are local signals, never a mandatory skeleton.

## Wikitext envelope

A Main-namespace article normally uses this order when each element is applicable:

```wikitext
{{SHORTDESC:Specific one-line description of the subject}}
{{Article quality|unverified}}
{{Infobox ...
|...
}}

'''Article title''' is [direct, supported definition].

== Subject-derived heading ==
[Encyclopedic prose with inline citations.]

== References ==
{{Reflist}}

[[Category:Specific existing category]]
```

The bracketed text is explanatory, not placeholder content to emit.

- Keep `SHORTDESC` concise and describe the subject, not “an article about” it.
- Use `unverified` for a new draft. Preserve existing `wip` or `verified` unless a human editor
  explicitly changes the review state.
- Put an applicable infobox after the quality banner and before the lead.
- The lead has no heading. Bold the title only at its first occurrence.
- Use sentence-case headings, `==` for sections, and `===` for subsections.
- Add `References` when inline citations exist. Add `See also`, `Further reading`, `External links`,
  or `Notes` only when they contain useful, non-duplicative material.
- Put specific verified categories at the end. Frequency in nearby pages is not a category rule.

## Deriving sections

Create a section when all of these are true:

1. it answers a distinct reader question about the subject;
2. it contains more than a repeated lead claim or one isolated detail;
3. its claims have adequate evidence;
4. the heading describes the content without evaluation;
5. separating it improves the article's flow.

Otherwise merge the material into a related section or omit it.

Useful organizing relationships include chronology, works, production, ideas, design, activity,
governance, participation, and documented reception. These are prompts for reasoning, not default
headings. For a person, do not invent “Early life” when no sourced early-life material exists. For
an artwork, do not create “Reception” from the agent's own response. For a project, do not turn a
feature list into prose sections without explaining how the features matter to the subject.

Write the body before the lead so the lead summarizes the article that exists rather than the
article imagined at the start.

## Comparable pages and interview plans

Use comparable pages to learn target vocabulary, typical article scale, link conventions, and
possible omissions. Do not copy their heading sequence or reproduce their weaknesses. The exact
subject page is excluded from comparable selection in the current `article-start` contract.

An interview brief may contain a draft plan. Treat it as an editor proposal to test against the
evidence. A proposed section can be rewritten, merged, reordered, or dropped. The agent may write
the resulting prose when the claim-source map is adequate.

Generic “Impact”, “Legacy”, “Future”, “Conclusion”, “Broader context”, and relationship-to-Remilia
sections deserve particular skepticism. Use one only when distinct, sourced material makes it the
clearest organization.

## Leads and article length

The lead identifies the subject and summarizes the body in proportion to its importance. It should
not carry a second mini-article about Remilia, Charlotte Fang, an industry, or a scene merely to
explain why the page was created.

Let evidence determine length. A short article can contain a definition, a compact chronology or
description, and sources. Do not expand it with generic background or interpretation to imitate a
larger encyclopedia entry. If the subject cannot yet sustain a standalone explanation, consider a
redirect or a section in a broader page.

## Infoboxes, media, and categories

An infobox summarizes supported facts; it does not replace the lead or authorize fields whose
values are unknown. Query the live contract before using a template:

```bash
wikitool templates show "Template:Infobox person" --format json --view brief
wikitool templates examples "Template:Infobox person" --limit 2
```

Use `parent_group = Remilia` only for an actual Remilia project when the template supports it. Do
not infer an individual's employment, identity, authorship, or relationship from a profile default.

Images should help identify or understand the subject. Captions state what the image shows and the
context needed to interpret it; they do not add unsupported criticism. Read `visual_subjects.md`
for artifact-description boundaries.

Choose categories from existing target-wiki categories because they improve navigation. There is
no universal `[[Category:Remilia]]` default and no requirement to reach an arbitrary category count.

## Rendering and extensions

Read `extensions.md` and the live wiki profile before adding extension syntax. Do not add raw
JavaScript or generated HTML to an article. For templates or Cargo-backed rendering, validate the
relevant rendered consumer shapes with `wiki render-check` after publication.
