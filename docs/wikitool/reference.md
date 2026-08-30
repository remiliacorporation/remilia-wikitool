# Wikitool Command Reference

This file is generated from Rust CLI help output. Do not edit manually.

Maintainer-only commands hidden from default help are intentionally omitted.

Regenerate from a source checkout with the maintainer surface enabled:

```bash
cargo run --package wikitool --features maintainer -- docs generate-reference
```

## Global

```text
Wiki management CLI

Usage: wikitool [OPTIONS] [COMMAND]

Commands:
  init        Initialize a new wikitool project
  config      Show resolved configuration and target-wiki sources
  pull        Pull wiki content and templates to local files
  push        Push local changes to the live wiki
  diff        Show local changes not yet pushed to the wiki
  status      Show sync status and local project state
  validate    Run structural and link integrity checks
  review      Run the structured pre-push review gate
  module      Run Lua module linting and related checks
  export      Export a remote wiki page tree to local files
  delete      Delete a page from the live wiki
  mutation    Inspect and reconcile durable remote mutation receipts
  db          Inspect or reset the local runtime database
  docs        Manage and query pinned MediaWiki docs corpora
  import      Import content from external sources
  catalog     Build and query the disposable local catalog
  adapter     Inspect the explicit project-owned site adapter
  interview   Create, validate, show, and audit neutral article interview ledgers
  source      Inspect target-wiki evidence and fetch source URLs without mutating the wiki
  wiki        Sync and inspect live wiki capability metadata
  templates   Build and inspect the local template catalog
  article     Lint and mechanically remediate article drafts
  lsp         Generate parser config and editor integration settings
  companions  Inspect optional release companions without changing their lifecycle state
  skills      Inspect and install Wikitool skills
  help        Print this message or the help of the given subcommand(s)

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
      --wiki-url <URL>       Target wiki base URL
      --api-url <URL>        Target MediaWiki API URL
      --data-dir <PATH>
      --adapter-path <PATH>  Project-relative site-adapter path to validate and store in project config
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --templates            Create templates/ during initialization
      --force                Overwrite existing config/parser files
      --no-config            Skip writing .wikitool/config.toml
      --no-parser-config     Skip writing parser config
      --no-network           Skip network namespace discovery during initialization
  -h, --help                 Print help
```

## config

```text
Show resolved configuration and target-wiki sources

Usage: wikitool config [OPTIONS] <COMMAND>

Commands:
  show  Show resolved configuration, paths, and target-wiki sources
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## config show

```text
Show resolved configuration, paths, and target-wiki sources

Usage: wikitool config show [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
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

Usage: wikitool push [OPTIONS] --summary <TEXT>

Options:
      --project-root <PATH>
      --summary <TEXT>       Edit summary for the bound push plan
      --apply <PLAN_ID>      Apply the exact plan ID returned by a current preview; without this option the command only previews
      --data-dir <PATH>
      --config <PATH>
      --force                Force push even when remote timestamps diverge
      --delete               Propagate local deletions to remote wiki pages
      --diagnostics          Print resolved runtime diagnostics
      --templates            Include template/module/mediawiki namespaces
      --categories           Limit push to Category namespace pages
      --all                  Explicitly include every eligible current change (cannot be combined with title/path selection)
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

## validate

```text
Run structural and link integrity checks

Usage: wikitool validate [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json; text exits non-zero on findings, json reports findings via status [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --summary              Omit detailed issue lists and print category counts
      --category <CATEGORY>  Limit validation to one issue category; repeat for multiple categories [possible values: broken-links, double-redirects, uncategorized-pages, orphan-pages]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --limit <N>            Limit issues returned per selected category
      --title <TITLE>        Limit issues to a page title
      --verify-live          Verify selected broken links and redirect issues against the live wiki API
      --advisory             Report validation issues without exiting non-zero
  -h, --help                 Print help
```

## review

```text
Run the structured pre-push review gate

Usage: wikitool review [OPTIONS]

Options:
      --format <FORMAT>          Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>              JSON view: brief|full [default: brief] [possible values: brief, full]
      --config <PATH>
      --strict                   Treat article lint warnings as review failures
      --diagnostics              Print resolved runtime diagnostics
      --templates                Include template/module/mediawiki namespaces in sync checks
      --categories               Limit sync checks to Category namespace pages
      --title <TITLE>
      --path <PATH>
      --draft-path <PATH>        Review one off-wiki draft path under .wikitool/drafts/; requires exactly one --title and skips the push preview
      --brief-path <PATH>        Validate and include an article interview brief in the review gate
      --brief-stale-days <DAYS>  Age in days after which an interview brief is considered stale [default: 45]
      --titles-file <PATH>       Read one canonical page title per line
      --summary <TEXT>           Edit summary bound into the push preview plan [default: "wikitool review preview"]
  -h, --help                     Print help
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

## export

```text
Export a remote wiki page tree to local files

Usage: wikitool export [OPTIONS] [URL]

Arguments:
  [URL]

Options:
      --project-root <PATH>
      --urls-file <PATH>      Read arbitrary source URLs from a newline-delimited file
      --data-dir <PATH>
  -o, --output <PATH>         Output file or directory path
      --config <PATH>
      --output-dir <DIR>      Output directory for URL batch, single-page, or separate subpage exports
      --diagnostics           Print resolved runtime diagnostics
      --format <FORMAT>       Output format: markdown|wikitext [default: markdown] [possible values: markdown, wikitext]
      --code-language <LANG>  Code language hint (reserved for markdown export)
      --no-frontmatter        Skip YAML frontmatter
      --subpages              Include subpages for MediaWiki page exports
      --combined              With --subpages, combine all pages into one output
      --limit <N>             Maximum total pages to export with --subpages, including the parent page
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
      --backup-dir <PATH>    Custom backup directory under .wikitool/sync/
      --config <PATH>
      --apply <PLAN_ID>      Apply the exact target/content/revision-bound plan ID; without this option the command only previews
      --diagnostics          Print resolved runtime diagnostics
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## mutation

```text
Inspect and reconcile durable remote mutation receipts

Usage: wikitool mutation [OPTIONS] <COMMAND>

Commands:
  list       List target-bound durable remote mutation receipts
  show       Show one target-bound remote mutation receipt
  reconcile  Reconcile one mutation without replaying its write
  close      Close an irreconcilable mutation without claiming a remote outcome
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## mutation list

```text
List target-bound durable remote mutation receipts

Usage: wikitool mutation list [OPTIONS]

Options:
      --all                  Include terminal receipts as well as unresolved mutations
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## mutation show

```text
Show one target-bound remote mutation receipt

Usage: wikitool mutation show [OPTIONS] <OPERATION> <MUTATION_ID>

Arguments:
  <OPERATION>    [possible values: edit, delete]
  <MUTATION_ID>

Options:
      --format <FORMAT>      [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## mutation reconcile

```text
Reconcile one mutation without replaying its write

Usage: wikitool mutation reconcile [OPTIONS] <OPERATION> <MUTATION_ID>

Arguments:
  <OPERATION>    [possible values: edit, delete]
  <MUTATION_ID>

Options:
      --format <FORMAT>      [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## mutation close

```text
Close an irreconcilable target-bound mutation, retain its evidence, and invalidate the title's sync baseline. A fresh target-bound pull is required before another write.

Usage: wikitool mutation close [OPTIONS] --actor <ACTOR> --reason <REASON> <OPERATION> <MUTATION_ID>

Arguments:
  <OPERATION>
          [possible values: edit, delete]

  <MUTATION_ID>


Options:
      --actor <ACTOR>
          Operator recording the closure

      --project-root <PATH>


      --data-dir <PATH>


      --reason <REASON>
          Reason remote truth cannot be proved

      --config <PATH>


      --confirm
          Confirm evidence-preserving closure and sync-baseline invalidation

      --diagnostics
          Print resolved runtime diagnostics

      --format <FORMAT>
          [default: text]
          [possible values: text, json]

  -h, --help
          Print help (see a summary with '-h')
```

## db

```text
Inspect or reset the local runtime database

Usage: wikitool db [OPTIONS] <COMMAND>

Commands:
  stats  Show local database state and catalog readiness
  reset  Delete the disposable local catalog database
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
Show local database state and catalog readiness

Usage: wikitool db stats [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db reset

```text
Delete the disposable local catalog database

Usage: wikitool db reset [OPTIONS]

Options:
      --project-root <PATH>
      --yes                  Assume yes and delete the catalog database without prompting
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
  [PROFILE]  Docs profile to hydrate (default: configured site adapter)

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
      --type <TYPE>          Filter technical docs by type: hooks|config|api|manual|help [possible values: hooks, config, api, manual, help]
      --config <PATH>
      --kind <KIND>          Filter corpora by kind: extension|technical|profile [possible values: extension, technical, profile]
      --diagnostics          Print resolved runtime diagnostics
      --profile <PROFILE>    Filter corpora by source profile
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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
      --tier <TIER>          Search tier: page|section|symbol|example|extension|technical|profile [possible values: page, section, symbol, example, extension, technical, profile]
      --data-dir <PATH>
      --profile <PROFILE>    Docs profile to search (default: configured site adapter)
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
      --profile <PROFILE>            Docs profile for retrieval (default: configured site adapter)
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
      --profile <PROFILE>    Docs profile for lookup (default: configured site adapter)
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -l, --limit <LIMIT>        Limit result count [default: 20]
  -h, --help                 Print help
```

## import

```text
Import content from external sources

Usage: wikitool import [OPTIONS] <COMMAND>

Commands:
  cargo
  html-to-wikitext  Compile captured HTML through explicit source and target profiles
  help              Print this message or the help of the given subcommand(s)

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
      --type <TYPE>            Input type: csv|json [possible values: csv, json]
      --config <PATH>
      --template <NAME>        Template wrapper name
      --diagnostics            Print resolved runtime diagnostics
      --title-field <FIELD>    Field name to use as page title
      --title-prefix <PREFIX>  Prefix for generated page titles
      --category <NAME>        Category to add to generated pages
      --mode <MODE>            create|update|upsert [default: create] [possible values: create, update, upsert]
      --write                  Write files (default: dry-run)
      --format <FORMAT>        Output format: text|json [default: text] [possible values: text, json]
      --article-header         Add SHORTDESC + Article quality header in main namespace
      --no-meta                Omit metadata from JSON output
  -h, --help                   Print help
```

## import html-to-wikitext

```text
Compile captured HTML through explicit source and target profiles

Usage: wikitool import html-to-wikitext [OPTIONS] --source-profile <PATH> --target-profile <PATH> --canonical-title <TITLE> --canonical-url <URL> --source-key <KEY> --media-scope <SCOPE> --output <PATH> <PATH>

Arguments:
  <PATH>  Captured HTML input path

Options:
      --project-root <PATH>
      --source-profile <PATH>    Source interpretation profile
      --data-dir <PATH>
      --target-profile <PATH>    Target authoring profile
      --canonical-title <TITLE>  Canonical source page title
      --config <PATH>
      --canonical-url <URL>      Canonical source page URL
      --diagnostics              Print resolved runtime diagnostics
      --source-key <KEY>         Captured source evidence key
      --media-scope <SCOPE>      Target archive-media scope
      --media-inventory <PATH>   Optional captured media-reference inventory
      --output <PATH>            Project-scoped output path for exact compiled wikitext
      --format <FORMAT>          Output format: text|json [default: text] [possible values: text, json]
  -h, --help                     Print help
```

## catalog

```text
Build and query the disposable local catalog

Usage: wikitool catalog [OPTIONS] <COMMAND>

Commands:
  build      Rebuild the local content catalog
  warm       Build the content catalog and hydrate a docs profile
  status     Report catalog readiness and degradations
  contracts  Plan and search token-budgeted authoring contracts
  inspect    Inspect indexed catalog structures directly
  surface    Build the derived agent-facing template, module, asset, and extension surface
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog build

```text
Rebuild the local content catalog

Usage: wikitool catalog build [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog warm

```text
Build the content catalog and hydrate a docs profile

Usage: wikitool catalog warm [OPTIONS]

Options:
      --docs-profile <PROFILE>  Docs profile to hydrate (default: configured site adapter)
      --project-root <PATH>
      --data-dir <PATH>
      --docs-mode <MODE>        Docs hydration mode: missing|refresh|skip [default: missing] [possible values: missing, refresh, skip]
      --config <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --diagnostics             Print resolved runtime diagnostics
  -h, --help                    Print help
```

## catalog status

```text
Report catalog readiness and degradations

Usage: wikitool catalog status [OPTIONS]

Options:
      --docs-profile <PROFILE>  Docs profile to assess (default: configured site adapter)
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
  -h, --help                    Print help
```

## catalog contracts

```text
Plan and search token-budgeted authoring contracts

Usage: wikitool catalog contracts [OPTIONS] <COMMAND>

Commands:
  search  Search the indexed authoring contract graph
  plan    Plan contract traversal for a topic or draft
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog contracts search

```text
Search the indexed authoring contract graph

Usage: wikitool catalog contracts search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  Template/module/authoring surface query

Options:
      --limit <N>              [default: 16]
      --project-root <PATH>
      --data-dir <PATH>
      --token-budget <TOKENS>  [default: 900]
      --config <PATH>
      --profile <PROFILE>      Contract traversal profile: index|author|implementation [default: author] [possible values: index, author, implementation]
      --diagnostics            Print resolved runtime diagnostics
      --format <FORMAT>        Output format: text|json [default: json] [possible values: text, json]
  -h, --help                   Print help
```

## catalog contracts plan

```text
Plan contract traversal for a topic or draft

Usage: wikitool catalog contracts plan [OPTIONS] [TOPIC]

Arguments:
  [TOPIC]  Primary article topic/title for traversal

Options:
      --project-root <PATH>
      --stub-path <PATH>        Optional stub wikitext file used for template seeds
      --data-dir <PATH>
      --limit <N>               [default: 16]
      --config <PATH>
      --token-budget <TOKENS>   [default: 900]
      --diagnostics             Print resolved runtime diagnostics
      --profile <PROFILE>       Contract traversal profile: index|author|implementation [default: author] [possible values: index, author, implementation]
      --contract-query <QUERY>  Optional contract traversal query separate from TOPIC
      --format <FORMAT>         Output format: text|json [default: json] [possible values: text, json]
  -h, --help                    Print help
```

## catalog inspect

```text
Inspect indexed catalog structures directly

Usage: wikitool catalog inspect [OPTIONS] <COMMAND>

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

## catalog inspect stats

```text
Show index statistics

Usage: wikitool catalog inspect stats [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog inspect chunks

```text
Retrieve token-budgeted content chunks from indexed pages

Usage: wikitool catalog inspect chunks [OPTIONS] [TITLE]

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
      --view <VIEW>            JSON view: brief|full [default: brief] [possible values: brief, full]
      --diversify              Enable lexical de-duplication and diversification
      --no-diversify           Disable lexical de-duplication and diversification
  -h, --help                   Print help
```

## catalog inspect backlinks

```text
Show indexed pages that link to a title

Usage: wikitool catalog inspect backlinks [OPTIONS] <TITLE>

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

## catalog inspect templates

```text
Inspect active template usage and implementation references

Usage: wikitool catalog inspect templates [OPTIONS] [TEMPLATE]

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

## catalog inspect references

```text
Audit indexed references for cleanup work

Usage: wikitool catalog inspect references [OPTIONS] <COMMAND>

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

## catalog inspect references summary

```text
Show aggregate reference audit counts

Usage: wikitool catalog inspect references summary [OPTIONS]

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

## catalog inspect references list

```text
List individual indexed references

Usage: wikitool catalog inspect references list [OPTIONS]

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

## catalog inspect references duplicates

```text
Show strong duplicate reference groups

Usage: wikitool catalog inspect references duplicates [OPTIONS]

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

## catalog inspect orphans

```text
Show indexed pages with no backlinks

Usage: wikitool catalog inspect orphans [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog inspect empty-categories

```text
Show categories with no indexed members

Usage: wikitool catalog inspect empty-categories [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog surface

```text
Build the derived agent-facing template, module, asset, and extension surface

Usage: wikitool catalog surface [OPTIONS] <COMMAND>

Commands:
  sync  Refresh and show the derived authoring surface
  show  Show the current derived authoring surface
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## catalog surface sync

```text
Refresh and show the derived authoring surface

Usage: wikitool catalog surface sync [OPTIONS]

Options:
      --format <FORMAT>             Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>                 JSON view: brief|full [default: brief] [possible values: brief, full]
      --config <PATH>
      --template-limit <N>          [default: 64]
      --diagnostics                 Print resolved runtime diagnostics
      --template-example-limit <N>  [default: 2]
      --module-limit <N>            [default: 128]
      --asset-limit <N>             [default: 128]
      --extension-limit <N>         [default: 128]
      --extension-tag-limit <N>     [default: 128]
      --parser-function-limit <N>   [default: 128]
  -h, --help                        Print help
```

## catalog surface show

```text
Show the current derived authoring surface

Usage: wikitool catalog surface show [OPTIONS]

Options:
      --format <FORMAT>             Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>                 JSON view: brief|full [default: brief] [possible values: brief, full]
      --config <PATH>
      --template-limit <N>          [default: 64]
      --diagnostics                 Print resolved runtime diagnostics
      --template-example-limit <N>  [default: 2]
      --module-limit <N>            [default: 128]
      --asset-limit <N>             [default: 128]
      --extension-limit <N>         [default: 128]
      --extension-tag-limit <N>     [default: 128]
      --parser-function-limit <N>   [default: 128]
  -h, --help                        Print help
```

## adapter

```text
Inspect the explicit project-owned site adapter

Usage: wikitool adapter [OPTIONS] <COMMAND>

Commands:
  inspect  Inspect the explicitly configured site adapter and publication identity
  help     Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## adapter inspect

```text
Inspect the explicitly configured site adapter and publication identity

Usage: wikitool adapter inspect [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview

```text
Create, validate, show, and audit neutral article interview ledgers

Usage: wikitool interview [OPTIONS] <COMMAND>

Commands:
  init       Create a timestamped article interview ledger and sidecars
  validate   Validate an article interview ledger and sidecars
  show       Show an article interview ledger summary
  audit      Audit all article interview ledgers in the project
  open-item  Append or list structured interview open items
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview init

```text
Create a timestamped article interview ledger and sidecars

Usage: wikitool interview init [OPTIONS] <TITLE>

Arguments:
  <TITLE>  Article title or topic for the interview

Options:
      --intent <INTENT>               Interview intent: new|expand|audit|refresh [default: new] [possible values: new, expand, audit, refresh]
      --project-root <PATH>
      --agent <AGENT>                 Agent label for brief metadata
      --data-dir <PATH>
      --config <PATH>
      --no-scout                      Skip the local-context scout and create a blank ledger
      --diagnostics                   Print resolved runtime diagnostics
      --source-article <TITLE>        Existing article title this interview concerns
      --related-draft <PATH>          Related draft path to record in brief metadata
      --timestamp <YYYYMMDDTHHMMSSZ>  UTC ledger timestamp; defaults to current time
      --force                         Overwrite files if the timestamped brief already exists
      --format <FORMAT>               Output format: text|json [default: json] [possible values: text, json]
  -h, --help                          Print help
```

## interview validate

```text
Validate an article interview ledger and sidecars

Usage: wikitool interview validate [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to .brief.md interview brief

Options:
      --project-root <PATH>
      --stale-days <DAYS>    Age in days after which a brief is considered stale [default: 45]
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview show

```text
Show an article interview ledger summary

Usage: wikitool interview show [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to .brief.md interview brief

Options:
      --project-root <PATH>
      --stale-days <DAYS>    Age in days after which a brief is considered stale [default: 45]
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --view <VIEW>          JSON view: brief|full [default: brief] [possible values: brief, full]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview audit

```text
Audit all article interview ledgers in the project

Usage: wikitool interview audit [OPTIONS]

Options:
      --project-root <PATH>
      --stale-days <DAYS>    Age in days after which a brief is considered stale [default: 45]
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --view <VIEW>          JSON view: brief|full [default: brief] [possible values: brief, full]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview open-item

```text
Append or list structured interview open items

Usage: wikitool interview open-item [OPTIONS] <COMMAND>

Commands:
  add     Append a structured open item to an interview brief sidecar
  list    List structured open items for an interview brief
  update  Update an existing open item's status, note, or text
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview open-item add

```text
Append a structured open item to an interview brief sidecar

Usage: wikitool interview open-item add [OPTIONS] --kind <KIND> --text <TEXT> <PATH>

Arguments:
  <PATH>  Path to .brief.md interview brief

Options:
      --kind <KIND>                   Open item kind [possible values: rejected-source, inaccessible-source, disproven-link, source-wiki-only-template, rejected-category, scope-unresolved, stale-interview, privacy-exclusion, missing-source, user-followup-needed, do-not-assert, other]
      --project-root <PATH>
      --data-dir <PATH>
      --status <STATUS>               Open item status: open|resolved|rejected|deferred [default: open] [possible values: open, resolved, rejected, deferred]
      --config <PATH>
      --text <TEXT>                   Open item text
      --diagnostics                   Print resolved runtime diagnostics
      --item-id <ID>                  Explicit open item id
      --source-lead <VALUE>           Source lead associated with this open item; repeatable
      --notes <TEXT>                  Optional note
      --timestamp <YYYYMMDDTHHMMSSZ>  UTC item timestamp; defaults to current time
      --no-touch-brief                Do not update brief last_updated/freshness metadata
      --format <FORMAT>               Output format: text|json [default: json] [possible values: text, json]
  -h, --help                          Print help
```

## interview open-item list

```text
List structured open items for an interview brief

Usage: wikitool interview open-item list [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to .brief.md interview brief

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## interview open-item update

```text
Update an existing open item's status, note, or text

Usage: wikitool interview open-item update [OPTIONS] --item-id <ID> <PATH>

Arguments:
  <PATH>  Path to .brief.md interview brief

Options:
      --item-id <ID>                  Open item id to update
      --project-root <PATH>
      --data-dir <PATH>
      --status <STATUS>               New status: open|resolved|rejected|deferred [possible values: open, resolved, rejected, deferred]
      --config <PATH>
      --text <TEXT>                   Replace the open item text
      --diagnostics                   Print resolved runtime diagnostics
      --notes <TEXT>                  Replace the optional note
      --timestamp <YYYYMMDDTHHMMSSZ>  UTC timestamp; defaults to current time
      --no-touch-brief                Do not update brief last_updated/freshness metadata
      --format <FORMAT>               Output format: text|json [default: json] [possible values: text, json]
  -h, --help                          Print help
```

## source

```text
Inspect target-wiki evidence and fetch source URLs without mutating the wiki

Usage: wikitool source [OPTIONS] <COMMAND>

Commands:
  wiki-search          Search the configured wiki API for subject evidence
  fetch                Fetch readable reference material from a URL
  archive              Mirror raw web pages and requisites into a local manifest archive
  discover             Discover public machine-readable source surfaces for a URL
  session              Manage human-solved source access sessions
  mediawiki-templates  Inspect live template contracts used by a source MediaWiki page
  help                 Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source wiki-search

```text
Search the configured wiki API for subject evidence

Usage: wikitool source wiki-search [OPTIONS] <QUERY>

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

## source fetch

```text
Fetch readable reference material from a URL

Usage: wikitool source fetch [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --format <FORMAT>        Output format: wikitext|html|rendered-html [default: html] [possible values: wikitext, html, rendered-html]
      --project-root <PATH>
      --data-dir <PATH>
      --output <FORMAT>        Output wrapper: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --refresh                Refresh the source cache entry before returning output
      --diagnostics            Print resolved runtime diagnostics
      --no-cache               Bypass the source cache for this fetch
      --content-limit <CHARS>  Limit returned content characters; cached source content remains complete
      --no-content             Omit fetched content from output while keeping metadata and extract
      --no-discover            Skip machine-surface discovery when a fetch fails
      --discover-limit <N>     Limit machine-surface entries included with failed fetch diagnostics [default: 12]
  -h, --help                   Print help
```

## source archive

```text
Mirror raw web pages and requisites into a local manifest archive

Usage: wikitool source archive [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --output-dir <PATH>        Write archive files to this directory
      --project-root <PATH>
      --data-dir <PATH>
      --max-pages <N>            Maximum URLs to attempt [default: 1000]
      --config <PATH>
      --max-bytes <BYTES>        Maximum bytes to store for a single response [default: 50000000]
      --diagnostics              Print resolved runtime diagnostics
      --max-depth <N>            Maximum link depth from the seed URL (seed is depth 0) [default: 8]
      --max-total-bytes <BYTES>  Maximum total bytes to store across the whole crawl [default: 1000000000]
      --span-hosts               Allow crawling linked URLs outside the source host
      --no-page-requisites       Do not enqueue linked page requisites such as CSS image URLs
      --format <FORMAT>          Output format: text|json [default: json] [possible values: text, json]
  -h, --help                     Print help
```

## source discover

```text
Discover public machine-readable source surfaces for a URL

Usage: wikitool source discover [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --limit <N>            Limit machine-surface entries [default: 20]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source session

```text
Manage human-solved source access sessions

Usage: wikitool source session [OPTIONS] <COMMAND>

Commands:
  import  Import source-issued browser cookies for a domain
  list    List imported source access sessions without cookie values
  show    Show one imported source access session without cookie values
  clear   Clear one imported source access session
  prune   Remove expired source access sessions
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source session import

```text
Import source-issued browser cookies for a domain

Usage: wikitool source session import [OPTIONS] --cookies <PATH|-> <URL>

Arguments:
  <URL>

Options:
      --cookies <PATH|->       Read cookies from stdin (-) or an existing regular, non-symlink file; literal values are rejected
      --project-root <PATH>
      --data-dir <PATH>
      --user-agent <UA>        Pin the browser user-agent used when the cookies were obtained
      --config <PATH>
      --ttl-seconds <SECONDS>  Expire this local session after the supplied number of seconds
      --diagnostics            Print resolved runtime diagnostics
      --format <FORMAT>        Output format: text|json [default: json] [possible values: text, json]
  -h, --help                   Print help
```

## source session list

```text
List imported source access sessions without cookie values

Usage: wikitool source session list [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source session show

```text
Show one imported source access session without cookie values

Usage: wikitool source session show [OPTIONS] <DOMAIN>

Arguments:
  <DOMAIN>

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source session clear

```text
Clear one imported source access session

Usage: wikitool source session clear [OPTIONS] <DOMAIN>

Arguments:
  <DOMAIN>

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source session prune

```text
Remove expired source access sessions

Usage: wikitool source session prune [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## source mediawiki-templates

```text
Inspect live template contracts used by a source MediaWiki page

Usage: wikitool source mediawiki-templates [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --limit <N>              Maximum selected template pages and invocation samples to return [default: 16]
      --project-root <PATH>
      --content-limit <BYTES>  Maximum source bytes per selected template page preview [default: 2400]
      --data-dir <PATH>
      --config <PATH>
      --parameter-limit <N>    Maximum TemplateData parameters returned per selected template [default: 64]
      --diagnostics            Print resolved runtime diagnostics
      --template <TITLE>       Fetch an exact template page from the source wiki; may be repeated
      --refresh                Refresh the cached source MediaWiki template report before returning output
      --no-cache               Bypass the source MediaWiki template report cache
      --format <FORMAT>        Output format: text|json [default: json] [possible values: text, json]
  -h, --help                   Print help
```

## wiki

```text
Sync and inspect live wiki capability metadata

Usage: wikitool wiki [OPTIONS] <COMMAND>

Commands:
  capabilities  Sync and inspect live wiki capability manifests
  cargo         Query the live wiki's Cargo extension tables
  render-check  Validate rendered live HTML and scoped link contracts
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
  sync    Fetch and store the current live wiki capability manifest
  show    Show the last stored wiki capability manifest
  remote  Inspect a remote MediaWiki capability surface without storing it
  help    Print this message or the help of the given subcommand(s)

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

## wiki capabilities remote

```text
Inspect a remote MediaWiki capability surface without storing it

Usage: wikitool wiki capabilities remote [OPTIONS] <URL>

Arguments:
  <URL>

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --view <VIEW>          JSON view: summary|full [default: summary] [possible values: summary, full]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki cargo

```text
Query the live wiki's Cargo extension tables

Usage: wikitool wiki cargo [OPTIONS] <COMMAND>

Commands:
  tables  List the live wiki's Cargo tables
  fields  Show a live Cargo table's field schema (names, types, list markers)
  rows    Fetch rows from a live Cargo table
  count   Count rows in a live Cargo table
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki cargo tables

```text
List the live wiki's Cargo tables

Usage: wikitool wiki cargo tables [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki cargo fields

```text
Show a live Cargo table's field schema (names, types, list markers)

Usage: wikitool wiki cargo fields [OPTIONS] <TABLE>

Arguments:
  <TABLE>  Cargo table name

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki cargo rows

```text
Fetch rows from a live Cargo table

Usage: wikitool wiki cargo rows [OPTIONS] <TABLE>

Arguments:
  <TABLE>  Cargo table name

Options:
      --field <FIELD>        Field to select (repeat or comma-separate); defaults to the table's full schema
      --project-root <PATH>
      --data-dir <PATH>
      --where <CLAUSE>       Cargo where clause, e.g. collection='Example Collection'
      --config <PATH>
      --order-by <CLAUSE>    Cargo order_by clause
      --diagnostics          Print resolved runtime diagnostics
      --limit <N>            Maximum rows to return [default: 10]
      --offset <N>           Row offset [default: 0]
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## wiki cargo count

```text
Count rows in a live Cargo table

Usage: wikitool wiki cargo count [OPTIONS] <TABLE>

Arguments:
  <TABLE>  Cargo table name to count rows in

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki render-check

```text
Validate rendered live HTML and scoped link contracts

Usage: wikitool wiki render-check [OPTIONS] <TITLE>

Arguments:
  <TITLE>  Live wiki page title to render and inspect

Options:
      --project-root <PATH>
      --scope-class <CLASS>           Inspect each rendered element carrying this CSS class as one scope
      --data-dir <PATH>
      --expect-scopes <N>             Require exactly N matching scope elements
      --config <PATH>
      --require-interactive-link      Require every scope to contain a non-crawler interactive link
      --diagnostics                   Print resolved runtime diagnostics
      --require-href-contains <TEXT>  Require every scope to contain an interactive href with this text (repeatable)
      --require-link-class <CLASS>    Require every scope to contain an interactive link with this CSS class (repeatable)
      --require-page-image <FILE>     Require the live PageImages/Popups representative file
      --allow-literal-wikilinks       Do not fail when rendered page text contains literal [[...]] wikitext
      --format <FORMAT>               Output format: text|json [default: text] [possible values: text, json]
      --view <VIEW>                   JSON view: brief|full [default: brief] [possible values: brief, full]
  -h, --help                          Print help
```

## templates

```text
Build and inspect the local template catalog

Usage: wikitool templates [OPTIONS] <COMMAND>

Commands:
  catalog   Build and store the local template catalog artifact
  show      Show one template catalog entry
  examples  Show example invocations for one template
  closure   Export an exact named template/module dependency closure
  contract  Validate and compare declarative template contracts
  scaffold  Preview or apply a contract-bound template scaffold
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
      --view <VIEW>          JSON view: brief|full [default: brief] [possible values: brief, full]
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

## templates closure

```text
Export an exact named template/module dependency closure

Usage: wikitool templates closure [OPTIONS] <TEMPLATE>...

Arguments:
  <TEMPLATE>...

Options:
      --max-nodes <N>        Fail if the transitive template/module closure exceeds this node count [default: 128]
      --project-root <PATH>
      --data-dir <PATH>
      --output <PATH>        Write the full JSON closure atomically inside the project root
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates contract

```text
Validate and compare declarative template contracts

Usage: wikitool templates contract [OPTIONS] <COMMAND>

Commands:
  capture       Capture an observed local template as an unapproved contract starter
  check         Validate a template contract and assess compatibility
  render-check  Execute contract render fixtures through the configured MediaWiki parser
  help          Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates contract capture

```text
Capture an observed local template as an unapproved contract starter

Usage: wikitool templates contract capture [OPTIONS] --output <PATH> <TEMPLATE>

Arguments:
  <TEMPLATE>

Options:
      --output <PATH>        Write a new project-scoped contract starter; existing files are refused
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates contract check

```text
Validate a template contract and assess compatibility

Usage: wikitool templates contract check [OPTIONS] <CONTRACT>

Arguments:
  <CONTRACT>

Options:
      --against <TEMPLATE>   Compare with a specific catalog template instead of the contract title
      --project-root <PATH>
      --data-dir <PATH>
      --output <PATH>        Write the full assessment atomically inside the project root
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates contract render-check

```text
Execute contract render fixtures through the configured MediaWiki parser

Usage: wikitool templates contract render-check [OPTIONS] <CONTRACT>

Arguments:
  <CONTRACT>

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## templates scaffold

```text
Preview or apply a contract-bound template scaffold

Usage: wikitool templates scaffold [OPTIONS] --output <PATH> <CONTRACT>

Arguments:
  <CONTRACT>

Options:
      --output <PATH>        Exact project-scoped template output path
      --project-root <PATH>
      --apply <PLAN_ID>      Apply the exact content/path/current-state-bound scaffold plan
      --data-dir <PATH>
      --config <PATH>
      --overwrite            Authorize replacing an existing different file during apply
      --diagnostics          Print resolved runtime diagnostics
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## article

```text
Lint and mechanically remediate article drafts

Usage: wikitool article [OPTIONS] <COMMAND>

Commands:
  scout      Assemble a typed retrieval-context packet for an article topic
  accept     Record a hash-bound acceptance decision in the local ledger
  changeset  Prepare or accept an exact-content multi-article review changeset
  lint       Lint article wikitext against MediaWiki and site-adapter rules
  fix        Apply safe mechanical fixes to article wikitext
  promote    Promote a draft with current transactional publication acceptance
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## article scout

```text
Assemble a typed retrieval-context packet for an article topic

Usage: wikitool article scout [OPTIONS] [TOPIC]

Arguments:
  [TOPIC]  Primary article topic/title for retrieval

Options:
      --project-root <PATH>
      --stub-path <PATH>            Optional stub wikitext file used for link/template hint extraction
      --brief-path <PATH>           Optional wiki interview ledger to validate and include in the scout packet
      --data-dir <PATH>
      --brief-stale-days <DAYS>     Age in days after which an interview ledger is considered stale [default: 45]
      --config <PATH>
      --diagnostics                 Print resolved runtime diagnostics
      --related-limit <N>           Maximum related pages [default: 18]
      --chunk-limit <N>             Maximum retrieved context chunks [default: 10]
      --token-budget <TOKENS>       Token budget across retrieved chunks [default: 1200]
      --max-pages <N>               Maximum distinct source pages in chunk retrieval [default: 8]
      --link-limit <N>              Maximum internal-link observations [default: 18]
      --category-limit <N>          Maximum category observations [default: 8]
      --template-limit <N>          Maximum template summaries [default: 16]
      --docs-profile <PROFILE>      Docs profile for bridged retrieval (default: configured site adapter)
      --contract-profile <PROFILE>  Contract traversal profile: index|author|implementation [default: author] [possible values: index, author, implementation]
      --contract-query <QUERY>      Optional contract traversal query separate from TOPIC
      --format <FORMAT>             Output format: text|json [default: json] [possible values: text, json]
      --view <VIEW>                 JSON view: brief|full [default: brief] [possible values: brief, full]
      --intent <INTENT>             Authoring intent: new|expand|audit|refresh [default: new] [possible values: new, expand, audit, refresh]
      --diversify                   Enable lexical chunk de-duplication and diversification
      --no-diversify                Disable lexical chunk de-duplication and diversification
  -h, --help                        Print help
```

## article accept

```text
Record a hash-bound acceptance decision in the local ledger

Usage: wikitool article accept [OPTIONS] --title <TITLE> --human-editor <IDENTITY> --prose-origin <ORIGIN> <PATH>

Arguments:
  <PATH>  Draft or Main-namespace article path whose exact prose was read

Options:
      --project-root <PATH>
      --title <TITLE>            Canonical Main-namespace article title
      --data-dir <PATH>
      --human-editor <IDENTITY>  Self-reported name or handle of the human editor; Wikitool does not authenticate it
      --config <PATH>
      --prose-origin <ORIGIN>    Prose origin: human-draft|human-revision|agent-draft|collaborative-draft|mechanical-conversion-of-human-prose|human-reviewed-legacy [possible values: human-draft, human-revision, agent-draft, collaborative-draft, mechanical-conversion-of-human-prose, human-reviewed-legacy]
      --allow-warnings           Record explicit caller acceptance of remaining lint warnings
      --diagnostics              Print resolved runtime diagnostics
      --format <FORMAT>          Output format: text|json [default: text] [possible values: text, json]
  -h, --help                     Print help
```

## article changeset

```text
Prepare or accept an exact-content multi-article review changeset

Usage: wikitool article changeset [OPTIONS] <COMMAND>

Commands:
  prepare  Freeze selected articles, lint evidence, and prose origin in a JSON manifest
  accept   Bind one named human decision to every exact item in a prepared manifest
  help     Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## article changeset prepare

```text
Freeze selected articles, lint evidence, and prose origin in a JSON manifest

Usage: wikitool article changeset prepare [OPTIONS] --output <PATH> --prose-origin <ORIGIN> [PATH]

Arguments:
  [PATH]  One Main-namespace article path, or one state-draft path paired with exactly one --title

Options:
      --output <PATH>          Write the prepared JSON manifest to this project-scoped path
      --project-root <PATH>
      --data-dir <PATH>
      --prose-origin <ORIGIN>  Truthful prose origin shared by every item in this changeset [possible values: human-draft, human-revision, agent-draft, collaborative-draft, mechanical-conversion-of-human-prose, human-reviewed-legacy]
      --config <PATH>
      --replace                Replace an existing manifest after reviewing its path
      --diagnostics            Print resolved runtime diagnostics
      --title <TITLE>
      --path <PATH>
      --titles-file <PATH>     Read one canonical page title per line
      --changed                Prepare the current changed Main-namespace article set
      --format <FORMAT>        Output format: text|json [default: text] [possible values: text, json]
  -h, --help                   Print help
```

## article changeset accept

```text
Bind one named human decision to every exact item in a prepared manifest

Usage: wikitool article changeset accept [OPTIONS] --human-editor <IDENTITY> <MANIFEST>

Arguments:
  <MANIFEST>  Prepared article review changeset JSON manifest

Options:
      --human-editor <IDENTITY>  Self-reported name or handle of the human editor; Wikitool does not authenticate it
      --project-root <PATH>
      --data-dir <PATH>
      --warnings <DECISION>      Warning decision: require-none|accept [default: require-none] [possible values: require-none, accept]
      --config <PATH>
      --format <FORMAT>          Output format: text|json [default: text] [possible values: text, json]
      --diagnostics              Print resolved runtime diagnostics
  -h, --help                     Print help
```

## article lint

```text
Lint article wikitext against MediaWiki and site-adapter rules

Usage: wikitool article lint [OPTIONS] [PATH]

Arguments:
  [PATH]  Article path; state-draft paths under .wikitool/drafts/ may use --title override

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --strict               Treat warnings as errors
      --config <PATH>
      --title <TITLE>        Select a canonical article title; with one .wikitool/drafts/ PATH, override the draft title
      --diagnostics          Print resolved runtime diagnostics
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
  [PATH]  Article path; state-draft paths under .wikitool/drafts/ may use --title override

Options:
      --apply <MODE>         Apply mode: none|safe [default: none] [possible values: none, safe]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --title <TITLE>        Select a canonical article title; with one .wikitool/drafts/ PATH, override the draft title
      --diagnostics          Print resolved runtime diagnostics
      --path <PATH>
      --titles-file <PATH>   Read one canonical page title per line
      --changed              Fix the current changed main-namespace article set
  -h, --help                 Print help
```

## article promote

```text
Promote a draft with current transactional publication acceptance

Usage: wikitool article promote [OPTIONS] --title <TITLE> <PATH>

Arguments:
  <PATH>  Human-accepted state-draft path under the canonical .wikitool/drafts/ directory

Options:
      --project-root <PATH>
      --title <TITLE>        Canonical article title for the destination under wiki_content/
      --data-dir <PATH>
      --overwrite            Overwrite the destination file if it already exists
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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

## companions

```text
Inspect optional release companions without changing their lifecycle state

Usage: wikitool companions [OPTIONS]

Options:
      --manifest <PATH>      Inspect this release-companions.json instead of the file beside the executable
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## skills

```text
Inspect and install Wikitool skills

Usage: wikitool skills [OPTIONS] <COMMAND>

Commands:
  inspect            Validate and describe a Wikitool skills distribution
  setup-project      Install Wikitool skills into a project
  uninstall-project  Remove an unchanged receipt-owned Wikitool skill installation
  help               Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## skills inspect

```text
Validate and describe a Wikitool skills distribution

Usage: wikitool skills inspect [OPTIONS] [PROJECT]

Arguments:
  [PROJECT]  Also inspect this project's install receipt

Options:
      --skills-root <PATH>  Skills root (default: skills/ beside the executable)
      --data-dir <PATH>
      --format <FORMAT>     [default: json] [possible values: text, json]
      --config <PATH>
      --diagnostics         Print resolved runtime diagnostics
  -h, --help                Print help
```

## skills setup-project

```text
Install Wikitool skills into a project

Usage: wikitool skills setup-project [OPTIONS] [PROJECT]

Arguments:
  [PROJECT]

Options:
      --skills-root <PATH>           Skills root (default: skills/ beside the executable)
      --data-dir <PATH>
      --skill-target <SKILL_TARGET>  [default: auto] [possible values: auto, agents, claude, both]
      --config <PATH>
      --dry-run                      Validate and print the exact plan without changing files
      --diagnostics                  Print resolved runtime diagnostics
      --format <FORMAT>              [default: text] [possible values: text, json]
  -h, --help                         Print help
```

## skills uninstall-project

```text
Remove an unchanged receipt-owned Wikitool skill installation

Usage: wikitool skills uninstall-project [OPTIONS] [PROJECT]

Arguments:
  [PROJECT]

Options:
      --dry-run          Validate and print the exact plan without changing files
      --data-dir <PATH>
      --format <FORMAT>  [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics      Print resolved runtime diagnostics
  -h, --help             Print help
```
