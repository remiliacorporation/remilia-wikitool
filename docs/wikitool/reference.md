# Wikitool Command Reference

This file is generated from Rust CLI help output. Do not edit manually.

Maintainer-only commands hidden from default help are intentionally omitted.

Regenerate from a source checkout with the maintainer surface enabled:

```bash
wikitool docs generate-reference
```

## Global

```text
Wiki management CLI

Usage: wikitool [OPTIONS] [COMMAND]

Commands:
  init       Initialize a new wikitool project
  pull       Pull wiki content and templates to local files
  push       Push local changes to the live wiki
  diff       Show local changes not yet pushed to the wiki
  status     Show sync status and local project state
  context    Show indexed local page context for one title
  search     Search indexed local page titles
  validate   Run structural and link integrity checks
  module     Run Lua module linting and related checks
  fetch      Fetch a remote URL as wikitext or rendered HTML
  export     Export a remote wiki page tree to local files
  delete     Delete a page from the live wiki
  db         Inspect or reset the local runtime database
  docs       Manage and query pinned MediaWiki docs corpora
  seo        Inspect SEO metadata for wiki pages
  net        Inspect link network and page relationships
  import     Import content from external sources
  knowledge  Build and query the local knowledge layer
  research   Search and fetch subject evidence without mutating the wiki
  wiki       Sync and inspect live wiki capability metadata
  templates  Build and inspect the local template catalog
  article    Lint and mechanically remediate article drafts
  lsp        Generate parser config and editor integration settings
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --license              Print license information and exit
  -h, --help                 Print help
  -V, --version              Print version
```

## init

```text
Initialize a new wikitool project

Usage: wikitool init [OPTIONS]

Options:
      --project-root <PATH>
      --templates            Create templates/ during initialization
      --data-dir <PATH>
      --force                Overwrite existing config/parser files
      --config <PATH>
      --no-config            Skip writing .wikitool/config.toml
      --diagnostics          Print resolved runtime diagnostics
      --no-parser-config     Skip writing parser config
  -h, --help                 Print help
```

## pull

```text
Pull wiki content and templates to local files

Usage: wikitool pull [OPTIONS]

Options:
      --full                 Full refresh (ignore last pull timestamp)
      --project-root <PATH>
      --data-dir <PATH>
      --overwrite-local      Overwrite locally modified files during pull
  -c, --category <NAME>      Filter by category
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --templates            Pull templates instead of articles
      --categories           Pull Category: namespace pages
      --all                  Pull everything (articles, categories, and templates)
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## push

```text
Push local changes to the live wiki

Usage: wikitool push [OPTIONS]

Options:
      --project-root <PATH>
      --summary <TEXT>       Edit summary for pushed changes
      --data-dir <PATH>
      --dry-run              Preview push actions without writing to the wiki
      --config <PATH>
      --force                Force push even when remote timestamps diverge
      --delete               Propagate local deletions to remote wiki pages
      --diagnostics          Print resolved runtime diagnostics
      --templates            Include template/module/mediawiki namespaces
      --categories           Limit push to Category namespace pages
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## diff

```text
Show local changes not yet pushed to the wiki

Usage: wikitool diff [OPTIONS]

Options:
      --project-root <PATH>
      --templates            Include template/module/mediawiki namespaces
      --categories           Limit diff to Category namespace pages
      --data-dir <PATH>
      --config <PATH>
      --verbose              Show hash-level details for modified entries
      --content              Render unified textual diffs against the last synced baseline
      --diagnostics          Print resolved runtime diagnostics
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## status

```text
Show sync status and local project state

Usage: wikitool status [OPTIONS]

Options:
      --modified             Only show modified
      --project-root <PATH>
      --conflicts            Only show conflicts
      --data-dir <PATH>
      --config <PATH>
      --templates            Include templates
      --categories           Limit status to Category namespace pages
      --diagnostics          Print resolved runtime diagnostics
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## context

```text
Show indexed local page context for one title

Usage: wikitool context [OPTIONS] <TITLE>

Arguments:
  <TITLE>

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## search

```text
Search indexed local page titles

Usage: wikitool search [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## validate

```text
Run structural and link integrity checks

Usage: wikitool validate [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## module

```text
Run Lua module linting and related checks

Usage: wikitool module [OPTIONS] <COMMAND>

Commands:
  lint  Lint Lua modules
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## module lint

```text
Lint Lua modules

Usage: wikitool module lint [OPTIONS] [TITLE]

Arguments:
  [TITLE]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --strict               Treat warnings as errors
      --config <PATH>
      --no-meta              Omit metadata from JSON output
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## fetch

```text
Fetch a remote URL as wikitext or rendered HTML

Usage: wikitool fetch [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --format <FORMAT>      Output format: wikitext|html|rendered-html [default: wikitext]
      --project-root <PATH>
      --data-dir <PATH>
      --save                 Save output under reference/<source>/ in project root
      --config <PATH>
      --name <NAME>          Custom name for saved reference file
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## export

```text
Export a remote wiki page tree to local files

Usage: wikitool export [OPTIONS] <URL>

Arguments:
  <URL>

Options:
  -o, --output <PATH>         Output file or directory path
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>       Output format: markdown|wikitext [default: markdown]
      --code-language <LANG>  Code language hint (reserved for markdown export)
      --config <PATH>
      --diagnostics           Print resolved runtime diagnostics
      --no-frontmatter        Skip YAML frontmatter
      --subpages              Include subpages for MediaWiki page exports
      --combined              With --subpages, combine all pages into one output
  -h, --help                  Print help
```

## delete

```text
Delete a page from the live wiki

Usage: wikitool delete [OPTIONS] --reason <TEXT> <TITLE>

Arguments:
  <TITLE>

Options:
      --project-root <PATH>
      --reason <TEXT>        Reason for deletion (required)
      --data-dir <PATH>
      --no-backup            Skip backup (not recommended)
      --backup-dir <PATH>    Custom backup directory under .wikitool/
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --dry-run              Preview deletion without making changes
  -h, --help                 Print help
```

## db

```text
Inspect or reset the local runtime database

Usage: wikitool db [OPTIONS] <COMMAND>

Commands:
  stats  Show local database state and knowledge readiness
  reset  Delete the local runtime database
  help   Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db stats

```text
Show local database state and knowledge readiness

Usage: wikitool db stats [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db reset

```text
Delete the local runtime database

Usage: wikitool db reset [OPTIONS]

Options:
      --project-root <PATH>
      --yes                  Assume yes and delete the local database without prompting
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs

```text
Manage and query pinned MediaWiki docs corpora

Usage: wikitool docs [OPTIONS] <COMMAND>

Commands:
  import            Import docs from a bundle or extension source
  import-technical  Import a targeted technical docs slice
  import-profile    Hydrate a named docs profile
  list              List imported docs corpora
  update            Refresh outdated imported docs corpora
  remove            Remove an imported docs corpus
  search            Search pinned docs corpora by text
  context           Build focused docs context from pinned corpora
  symbols           Lookup docs symbols such as hooks, config vars, and APIs
  help              Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs import

```text
Import docs from a bundle or extension source

Usage: wikitool docs import [OPTIONS] [EXTENSION]...

Arguments:
  [EXTENSION]...

Options:
      --bundle <PATH>        Import docs from precomposed bundle JSON
      --project-root <PATH>
      --data-dir <PATH>
      --installed            Discover installed extensions from live wiki API
      --config <PATH>
      --no-subpages          Skip extension subpages
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs import-technical

```text
Import a targeted technical docs slice

Usage: wikitool docs import-technical [OPTIONS] [PAGE]...

Arguments:
  [PAGE]...

Options:
      --project-root <PATH>
      --subpages             Include subpages for selected pages/types
      --data-dir <PATH>
      --hooks                Import all hook documentation
      --config <PATH>
      --config-vars          Import configuration variable docs
      --api                  Import API documentation
      --diagnostics          Print resolved runtime diagnostics
      --help-docs            Import Help: docs
  -l, --limit <LIMIT>        Limit subpage imports per task [default: 100]
  -h, --help                 Print help
```

## docs import-profile

```text
Hydrate a named docs profile

Usage: wikitool docs import-profile [OPTIONS] [PROFILE]

Arguments:
  [PROFILE]  [default: remilia-mw-1.44]

Options:
      --installed              Discover installed extensions from the configured wiki
      --project-root <PATH>
      --data-dir <PATH>
      --no-extension-subpages  Skip extension subpages for profile extension docs
      --config <PATH>
      --extension <EXTENSION>  Add extra extension docs to the profile import
      --diagnostics            Print resolved runtime diagnostics
  -l, --limit <LIMIT>          Limit subpage imports per profile seed [default: 100]
  -h, --help                   Print help
```

## docs list

```text
List imported docs corpora

Usage: wikitool docs list [OPTIONS]

Options:
      --outdated             Show only outdated docs
      --project-root <PATH>
      --data-dir <PATH>
      --type <TYPE>          Filter technical docs by type
      --config <PATH>
      --kind <KIND>          Filter corpora by kind
      --diagnostics          Print resolved runtime diagnostics
      --profile <PROFILE>    Filter corpora by source profile
  -h, --help                 Print help
```

## docs update

```text
Refresh outdated imported docs corpora

Usage: wikitool docs update [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs remove

```text
Remove an imported docs corpus

Usage: wikitool docs remove [OPTIONS] <TARGET>

Arguments:
  <TARGET>

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs search

```text
Search pinned docs corpora by text

Usage: wikitool docs search [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --project-root <PATH>
      --tier <TIER>          Search tier: page|section|symbol|example|extension|technical|profile
      --data-dir <PATH>
      --profile <PROFILE>    Restrict search to a docs profile
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -l, --limit <LIMIT>        Limit result count [default: 20]
  -h, --help                 Print help
```

## docs context

```text
Build focused docs context from pinned corpora

Usage: wikitool docs context [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --profile <PROFILE>            Restrict context retrieval to a docs profile
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>              Output format: text|json [default: json] [possible values: text, json]
      --config <PATH>
  -l, --limit <LIMIT>                Limit hits per tier [default: 6]
      --diagnostics                  Print resolved runtime diagnostics
      --token-budget <TOKEN_BUDGET>  Approximate token budget for returned context [default: 1600]
  -h, --help                         Print help
```

## docs symbols

```text
Lookup docs symbols such as hooks, config vars, and APIs

Usage: wikitool docs symbols [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --kind <KIND>          Symbol kind filter
      --project-root <PATH>
      --data-dir <PATH>
      --profile <PROFILE>    Restrict symbol lookup to a docs profile
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -l, --limit <LIMIT>        Limit result count [default: 20]
  -h, --help                 Print help
```

## seo

```text
Inspect SEO metadata for wiki pages

Usage: wikitool seo [OPTIONS] <TARGET>

Arguments:
  <TARGET>

Options:
      --json                 Output JSON for AI consumption
      --project-root <PATH>
      --data-dir <PATH>
      --no-meta              Omit metadata from JSON output
      --config <PATH>
      --url <URL>            Override target URL
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## net

```text
Inspect link network and page relationships

Usage: wikitool net [OPTIONS] <TARGET>

Arguments:
  <TARGET>

Options:
      --limit <N>            Limit number of resources to probe [default: 25]
      --project-root <PATH>
      --data-dir <PATH>
      --no-probe             Skip HEAD probes (faster, no size/cache info)
      --config <PATH>
      --json                 Output JSON for AI consumption
      --diagnostics          Print resolved runtime diagnostics
      --no-meta              Omit metadata from JSON output
      --url <URL>            Override target URL
  -h, --help                 Print help
```

## import

```text
Import content from external sources

Usage: wikitool import [OPTIONS] <COMMAND>

Commands:
  cargo
  help   Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## import cargo

```text
Usage: wikitool import cargo [OPTIONS] --table <NAME> <PATH>

Arguments:
  <PATH>

Options:
      --project-root <PATH>
      --table <NAME>           Cargo table name
      --data-dir <PATH>
      --type <TYPE>            Input type: csv|json
      --config <PATH>
      --template <NAME>        Template wrapper name
      --diagnostics            Print resolved runtime diagnostics
      --title-field <FIELD>    Field name to use as page title
      --title-prefix <PREFIX>  Prefix for generated page titles
      --category <NAME>        Category to add to generated pages
      --mode <MODE>            create|update|upsert [default: create]
      --write                  Write files (default: dry-run)
      --format <FORMAT>        Output format: text|json [default: text] [possible values: text, json]
      --article-header         Add SHORTDESC + Article quality header in main namespace
      --no-meta                Omit metadata from JSON output
  -h, --help                   Print help
```

## knowledge

```text
Build and query the local knowledge layer

Usage: wikitool knowledge [OPTIONS] <COMMAND>

Commands:
  build          Rebuild the local content knowledge index
  warm           Build content knowledge and hydrate a docs profile
  status         Report knowledge readiness and degradations
  pack           Assemble the authoring knowledge pack
  article-start  Assemble an interpreted authoring brief for a topic
  inspect        Inspect indexed knowledge structures directly
  help           Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge build

```text
Rebuild the local content knowledge index

Usage: wikitool knowledge build [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge warm

```text
Build content knowledge and hydrate a docs profile

Usage: wikitool knowledge warm [OPTIONS]

Options:
      --docs-profile <PROFILE>  Docs profile to hydrate during warmup [default: remilia-mw-1.44]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
  -h, --help                    Print help
```

## knowledge status

```text
Report knowledge readiness and degradations

Usage: wikitool knowledge status [OPTIONS]

Options:
      --docs-profile <PROFILE>  Docs profile to assess for authoring readiness [default: remilia-mw-1.44]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
  -h, --help                    Print help
```

## knowledge pack

```text
Assemble the authoring knowledge pack

Usage: wikitool knowledge pack [OPTIONS] [TOPIC]

Arguments:
  [TOPIC]  Primary article topic/title for retrieval

Options:
      --project-root <PATH>
      --stub-path <PATH>        Optional stub wikitext file used for link/template hint extraction
      --data-dir <PATH>
      --related-limit <N>       Maximum related pages in the pack [default: 18]
      --chunk-limit <N>         Maximum retrieved context chunks [default: 10]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
      --token-budget <TOKENS>   Token budget across retrieved chunks [default: 1200]
      --max-pages <N>           Maximum distinct source pages in chunk retrieval [default: 8]
      --link-limit <N>          Maximum internal link suggestions [default: 18]
      --category-limit <N>      Maximum category suggestions [default: 8]
      --template-limit <N>      Maximum template summaries [default: 16]
      --docs-profile <PROFILE>  Docs profile to use for bridged authoring retrieval [default: remilia-mw-1.44]
      --format <FORMAT>         Output format: text|json [default: json] [possible values: text, json]
      --diversify               Enable lexical chunk de-duplication and diversification
      --no-diversify            Disable lexical chunk de-duplication and diversification
  -h, --help                    Print help
```

## knowledge article-start

```text
Assemble an interpreted authoring brief for a topic

Usage: wikitool knowledge article-start [OPTIONS] [TOPIC]

Arguments:
  [TOPIC]  Primary article topic/title for retrieval

Options:
      --project-root <PATH>
      --stub-path <PATH>        Optional stub wikitext file used for link/template hint extraction
      --data-dir <PATH>
      --related-limit <N>       Maximum related pages in the brief [default: 18]
      --chunk-limit <N>         Maximum retrieved context chunks [default: 10]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
      --token-budget <TOKENS>   Token budget across retrieved chunks [default: 1200]
      --max-pages <N>           Maximum distinct source pages in chunk retrieval [default: 8]
      --link-limit <N>          Maximum internal link suggestions [default: 18]
      --category-limit <N>      Maximum category suggestions [default: 8]
      --template-limit <N>      Maximum template summaries [default: 16]
      --docs-profile <PROFILE>  Docs profile to use for bridged authoring retrieval [default: remilia-mw-1.44]
      --format <FORMAT>         Output format: text|json [default: json] [possible values: text, json]
      --include-pack            Include the raw knowledge pack in JSON output
      --diversify               Enable lexical chunk de-duplication and diversification
      --no-diversify            Disable lexical chunk de-duplication and diversification
  -h, --help                    Print help
```

## knowledge inspect

```text
Inspect indexed knowledge structures directly

Usage: wikitool knowledge inspect [OPTIONS] <COMMAND>

Commands:
  stats             Show index statistics
  chunks            Retrieve token-budgeted content chunks from indexed pages
  backlinks         Show indexed pages that link to a title
  templates         Inspect active template usage and implementation references
  references        Audit indexed references for cleanup work
  orphans           Show indexed pages with no backlinks
  empty-categories  Show categories with no indexed members
  help              Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect stats

```text
Show index statistics

Usage: wikitool knowledge inspect stats [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect chunks

```text
Retrieve token-budgeted content chunks from indexed pages

Usage: wikitool knowledge inspect chunks [OPTIONS] [TITLE]

Arguments:
  [TITLE]

Options:
      --project-root <PATH>
      --query <QUERY>          Optional relevance query applied to chunk retrieval
      --across-pages           Retrieve chunks across indexed pages (query required, omit TITLE)
      --data-dir <PATH>
      --config <PATH>
      --limit <N>              Maximum number of chunks to return [default: 8]
      --diagnostics            Print resolved runtime diagnostics
      --token-budget <TOKENS>  Token budget across returned chunks [default: 720]
      --max-pages <N>          Maximum distinct source pages in across-pages mode [default: 12]
      --format <FORMAT>        Output format: text|json [default: text] [possible values: text, json]
      --diversify              Enable lexical de-duplication and diversification
      --no-diversify           Disable lexical de-duplication and diversification
  -h, --help                   Print help
```

## knowledge inspect backlinks

```text
Show indexed pages that link to a title

Usage: wikitool knowledge inspect backlinks [OPTIONS] <TITLE>

Arguments:
  <TITLE>

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect templates

```text
Inspect active template usage and implementation references

Usage: wikitool knowledge inspect templates [OPTIONS] [TEMPLATE]

Arguments:
  [TEMPLATE]  Optional specific template title

Options:
      --limit <N>            Maximum templates to return in catalog mode [default: 40]
      --project-root <PATH>
      --all                  Return the full active template catalog
      --data-dir <PATH>
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect references

```text
Audit indexed references for cleanup work

Usage: wikitool knowledge inspect references [OPTIONS] <COMMAND>

Commands:
  summary     Show aggregate reference audit counts
  list        List individual indexed references
  duplicates  Show strong duplicate reference groups
  help        Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect references summary

```text
Show aggregate reference audit counts

Usage: wikitool knowledge inspect references summary [OPTIONS]

Options:
      --project-root <PATH>

      --title <TITLE>

      --data-dir <PATH>

      --titles-file <PATH>
          Read one canonical page title per line
      --all
          Inspect all indexed pages
      --config <PATH>

      --diagnostics
          Print resolved runtime diagnostics
      --domain <DOMAIN>

      --template <TEMPLATE>

      --authority <AUTHORITY>

      --identifier-key <IDENTIFIER_KEY>

      --identifier <IDENTIFIER>

      --format <FORMAT>
          Output format: text|json [default: text] [possible values: text, json]
  -h, --help
          Print help
```

## knowledge inspect references list

```text
List individual indexed references

Usage: wikitool knowledge inspect references list [OPTIONS]

Options:
      --project-root <PATH>

      --title <TITLE>

      --data-dir <PATH>

      --titles-file <PATH>
          Read one canonical page title per line
      --all
          Inspect all indexed pages
      --config <PATH>

      --diagnostics
          Print resolved runtime diagnostics
      --domain <DOMAIN>

      --template <TEMPLATE>

      --authority <AUTHORITY>

      --identifier-key <IDENTIFIER_KEY>

      --identifier <IDENTIFIER>

      --format <FORMAT>
          Output format: text|json [default: text] [possible values: text, json]
  -h, --help
          Print help
```

## knowledge inspect references duplicates

```text
Show strong duplicate reference groups

Usage: wikitool knowledge inspect references duplicates [OPTIONS]

Options:
      --project-root <PATH>

      --title <TITLE>

      --data-dir <PATH>

      --titles-file <PATH>
          Read one canonical page title per line
      --all
          Inspect all indexed pages
      --config <PATH>

      --diagnostics
          Print resolved runtime diagnostics
      --domain <DOMAIN>

      --template <TEMPLATE>

      --authority <AUTHORITY>

      --identifier-key <IDENTIFIER_KEY>

      --identifier <IDENTIFIER>

      --format <FORMAT>
          Output format: text|json [default: text] [possible values: text, json]
  -h, --help
          Print help
```

## knowledge inspect orphans

```text
Show indexed pages with no backlinks

Usage: wikitool knowledge inspect orphans [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge inspect empty-categories

```text
Show categories with no indexed members

Usage: wikitool knowledge inspect empty-categories [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research

```text
Search and fetch subject evidence without mutating the wiki

Usage: wikitool research [OPTIONS] <COMMAND>

Commands:
  search  Search the remote wiki API for subject evidence
  fetch   Fetch readable reference material from a URL
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research search

```text
Search the remote wiki API for subject evidence

Usage: wikitool research search [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --limit <N>            [default: 20]
      --project-root <PATH>
      --data-dir <PATH>
      --what <SCOPE>         Search scope: text|title|nearmatch [default: text] [possible values: text, title, nearmatch]
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research fetch

```text
Fetch readable reference material from a URL

Usage: wikitool research fetch [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --format <FORMAT>      Output format: wikitext|html|rendered-html [default: html]
      --project-root <PATH>
      --data-dir <PATH>
      --output <FORMAT>      Output wrapper: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --refresh              Refresh the research cache entry before returning output
      --diagnostics          Print resolved runtime diagnostics
      --no-cache             Bypass the research cache for this fetch
  -h, --help                 Print help
```

## wiki

```text
Sync and inspect live wiki capability metadata

Usage: wikitool wiki [OPTIONS] <COMMAND>

Commands:
  capabilities  Sync and inspect live wiki capability manifests
  profile       Show the combined live/profile-aware wiki surface
  rules         Show the structured local editorial rules overlay
  help          Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki capabilities

```text
Sync and inspect live wiki capability manifests

Usage: wikitool wiki capabilities [OPTIONS] <COMMAND>

Commands:
  sync  Fetch and store the current live wiki capability manifest
  show  Show the last stored wiki capability manifest
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki capabilities sync

```text
Fetch and store the current live wiki capability manifest

Usage: wikitool wiki capabilities sync [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>          JSON view: summary|full [default: summary] [possible values: summary, full]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki capabilities show

```text
Show the last stored wiki capability manifest

Usage: wikitool wiki capabilities show [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>          JSON view: summary|full [default: summary] [possible values: summary, full]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki profile

```text
Show the combined live/profile-aware wiki surface

Usage: wikitool wiki profile [OPTIONS] <COMMAND>

Commands:
  sync  Refresh the local rules overlay and live capability snapshot
  show  Show the current combined profile snapshot
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki profile sync

```text
Refresh the local rules overlay and live capability snapshot

Usage: wikitool wiki profile sync [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>          JSON view: summary|full [default: summary] [possible values: summary, full]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki profile show

```text
Show the current combined profile snapshot

Usage: wikitool wiki profile show [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>          JSON view: summary|full [default: summary] [possible values: summary, full]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki rules

```text
Show the structured local editorial rules overlay

Usage: wikitool wiki rules [OPTIONS] <COMMAND>

Commands:
  show  Show the current Remilia rules overlay
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki rules show

```text
Show the current Remilia rules overlay

Usage: wikitool wiki rules show [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates

```text
Build and inspect the local template catalog

Usage: wikitool templates [OPTIONS] <COMMAND>

Commands:
  catalog   Build and store the local template catalog artifact
  show      Show one template catalog entry
  examples  Show example invocations for one template
  help      Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates catalog

```text
Build and store the local template catalog artifact

Usage: wikitool templates catalog [OPTIONS] <COMMAND>

Commands:
  build  Build the catalog from tracked templates plus local index usage
  help   Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates catalog build

```text
Build the catalog from tracked templates plus local index usage

Usage: wikitool templates catalog build [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates show

```text
Show one template catalog entry

Usage: wikitool templates show [OPTIONS] <TEMPLATE>

Arguments:
  <TEMPLATE>

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates examples

```text
Show example invocations for one template

Usage: wikitool templates examples [OPTIONS] <TEMPLATE>

Arguments:
  <TEMPLATE>

Options:
      --limit <N>            [default: 8]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## article

```text
Lint and mechanically remediate article drafts

Usage: wikitool article [OPTIONS] <COMMAND>

Commands:
  lint  Lint article wikitext against wiki/profile rules
  fix   Apply safe mechanical fixes to article wikitext
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## article lint

```text
Lint article wikitext against wiki/profile rules

Usage: wikitool article lint [OPTIONS] [PATH]

Arguments:
  [PATH]

Options:
      --profile <PROFILE>    [default: remilia]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --strict               Treat warnings as errors
      --diagnostics          Print resolved runtime diagnostics
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --changed              Lint the current changed main-namespace article set
  -h, --help                 Print help
```

## article fix

```text
Apply safe mechanical fixes to article wikitext

Usage: wikitool article fix [OPTIONS] [PATH]

Arguments:
  [PATH]

Options:
      --profile <PROFILE>    [default: remilia]
      --project-root <PATH>
      --apply <MODE>         Apply mode: none|safe [default: none]
      --data-dir <PATH>
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --changed              Fix the current changed main-namespace article set
  -h, --help                 Print help
```

## lsp

```text
Generate parser config and editor integration settings

Usage: wikitool lsp [OPTIONS] <COMMAND>

Commands:
  generate-config  Write parser config and print editor settings JSON
  status           Show parser config and runtime config status
  info             Show the preferred LSP integration entry point
  help             Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp generate-config

```text
Write parser config and print editor settings JSON

Usage: wikitool lsp generate-config [OPTIONS]

Options:
      --force                Overwrite parser config if it already exists
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp status

```text
Show parser config and runtime config status

Usage: wikitool lsp status [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp info

```text
Show the preferred LSP integration entry point

Usage: wikitool lsp info [OPTIONS]

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```
