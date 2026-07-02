# Wikitool Command Reference

This file is generated from Rust CLI help output. Do not edit manually.

Maintainer-only commands hidden from default help are intentionally omitted.

Regenerate from a source checkout with the maintainer surface enabled:

```bash
cargo run --features maintainer -- docs generate-reference
```

## Global

```text
Wiki management CLI

Usage: wikitool [OPTIONS] [COMMAND]

Commands:
  init       Initialize a new wikitool project
  config     Show resolved configuration and target-wiki sources
  pull       Pull wiki content and templates to local files
  push       Push local changes to the live wiki
  diff       Show local changes not yet pushed to the wiki
  status     Show sync status and local project state
  validate   Run structural and link integrity checks
  review     Run the structured pre-push review gate
  module     Run Lua module linting and related checks
  export     Export a remote wiki page tree to local files
  delete     Delete a page from the live wiki
  purge      Purge pages through the MediaWiki API
  upload     Upload a local file through the MediaWiki API
  move       Move (rename) a page through the MediaWiki API
  protect    Protect or unprotect a page through the MediaWiki API
  undelete   Restore a deleted page through the MediaWiki API
  db         Inspect or reset the local runtime database
  docs       Manage and query pinned MediaWiki docs corpora
  import     Import content from external sources
  knowledge  Build and query the local knowledge layer
  research   Inspect target-wiki evidence and fetch source URLs without mutating the wiki
  wiki       Sync and inspect live wiki capability metadata
  templates  Build and inspect the local template catalog
  article    Lint and mechanically remediate article drafts
  lsp        Generate parser config and editor integration settings
  workflow   First-run setup and session/full runtime refresh workflows
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
      --wiki-url <URL>       Target wiki base URL; defaults to https://wiki.remilia.org
      --api-url <URL>        Target MediaWiki API URL; defaults to https://wiki.remilia.org/api.php
      --data-dir <PATH>
      --config <PATH>
      --templates            Create templates/ during initialization
      --diagnostics          Print resolved runtime diagnostics
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
      --draft-path <PATH>        Review one off-wiki draft path under .wikitool/drafts/; requires exactly one --title and skips push dry-run
      --brief-path <PATH>        Validate and include a knowledge interview brief in the review gate
      --brief-stale-days <DAYS>  Age in days after which an interview brief is considered stale [default: 45]
      --titles-file <PATH>       Read one canonical page title per line
      --summary <TEXT>           Edit summary used for the push dry-run report [default: "wikitool review dry-run"]
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
      --backup-dir <PATH>    Custom backup directory under .wikitool/
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
      --dry-run              Preview deletion without making changes
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## purge

```text
Purge pages through the MediaWiki API

Usage: wikitool purge [OPTIONS] [TITLE]...

Arguments:
  [TITLE]...

Options:
      --project-root <PATH>
      --title <TITLE>
      --data-dir <PATH>
      --titles-file <PATH>        Read one canonical page title per line
      --config <PATH>
      --forcelinkupdate           Force link table update while purging
      --diagnostics               Print resolved runtime diagnostics
      --forcerecursivelinkupdate  Force recursive link table update while purging
      --dry-run                   Preview purge without writing to the wiki
      --format <FORMAT>           Output format: text|json [default: text] [possible values: text, json]
  -h, --help                      Print help
```

## upload

```text
Upload a local file through the MediaWiki API

Usage: wikitool upload [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --filename <FILENAME>  Target wiki filename
      --project-root <PATH>
      --comment <TEXT>       Upload comment [default: "Upload via wikitool"]
      --data-dir <PATH>
      --config <PATH>
      --text <WIKITEXT>      Initial file description text
      --diagnostics          Print resolved runtime diagnostics
      --ignore-warnings      Pass ignorewarnings=1 to MediaWiki upload
      --dry-run              Preview upload without writing to the wiki
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## move

```text
Move (rename) a page through the MediaWiki API

Usage: wikitool move [OPTIONS] <FROM> <TO>

Arguments:
  <FROM>
  <TO>

Options:
      --project-root <PATH>
      --reason <TEXT>        Move reason [default: "Move via wikitool"]
      --data-dir <PATH>
      --no-redirect          Do not leave a redirect at the old title (default leaves one)
      --config <PATH>
      --move-talk            Also move the associated talk page
      --diagnostics          Print resolved runtime diagnostics
      --move-subpages        Also move subpages (up to the API limit)
      --ignore-warnings      Pass ignorewarnings=1 to MediaWiki move
      --dry-run              Preview move without writing to the wiki
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
  -h, --help                 Print help
```

## protect

```text
Protect or unprotect a page through the MediaWiki API

Usage: wikitool protect [OPTIONS] --protection <TYPE=LEVEL> <TITLE>

Arguments:
  <TITLE>

Options:
      --project-root <PATH>
      --protection <TYPE=LEVEL>  Restriction to apply, e.g. edit=sysop or move=autoconfirmed; repeat for multiple. An empty level (edit=) clears the restriction
      --data-dir <PATH>
      --expiry <EXPIRY>          Protection expiry (MediaWiki timestamp or relative expression) [default: infinite]
      --config <PATH>
      --reason <TEXT>            Protection reason [default: "Protect via wikitool"]
      --diagnostics              Print resolved runtime diagnostics
      --dry-run                  Preview protect without writing to the wiki
      --format <FORMAT>          Output format: text|json [default: text] [possible values: text, json]
  -h, --help                     Print help
```

## undelete

```text
Restore a deleted page through the MediaWiki API

Usage: wikitool undelete [OPTIONS] <TITLE>

Arguments:
  <TITLE>

Options:
      --project-root <PATH>
      --reason <TEXT>        Undelete reason [default: "Undelete via wikitool"]
      --data-dir <PATH>
      --dry-run              Preview undelete without writing to the wiki
      --config <PATH>
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --diagnostics          Print resolved runtime diagnostics
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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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
  [PROFILE]  [default: remilia-wiki]

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

## knowledge

```text
Build and query the local knowledge layer

Usage: wikitool knowledge [OPTIONS] <COMMAND>

Commands:
  build          Rebuild the local content knowledge index
  warm           Build content knowledge and hydrate a docs profile
  status         Report knowledge readiness and degradations
  article-start  Assemble an interpreted authoring brief for a topic
  contracts      Plan and search token-budgeted authoring contracts
  interview      Create, validate, show, and audit knowledge interview briefs
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
      --docs-profile <PROFILE>  Docs profile to hydrate during warmup [default: remilia-wiki]
      --project-root <PATH>
      --data-dir <PATH>
      --docs-mode <MODE>        Docs hydration mode: missing|refresh|skip [default: missing] [possible values: missing, refresh, skip]
      --config <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --diagnostics             Print resolved runtime diagnostics
  -h, --help                    Print help
```

## knowledge status

```text
Report knowledge readiness and degradations

Usage: wikitool knowledge status [OPTIONS]

Options:
      --docs-profile <PROFILE>  Docs profile to assess for authoring readiness [default: remilia-wiki]
      --project-root <PATH>
      --data-dir <PATH>
      --format <FORMAT>         Output format: text|json [default: text] [possible values: text, json]
      --config <PATH>
      --diagnostics             Print resolved runtime diagnostics
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
      --stub-path <PATH>            Optional stub wikitext file used for link/template hint extraction
      --brief-path <PATH>           Optional knowledge interview brief to validate and include in the authoring brief
      --data-dir <PATH>
      --brief-stale-days <DAYS>     Age in days after which an interview brief is considered stale [default: 45]
      --config <PATH>
      --diagnostics                 Print resolved runtime diagnostics
      --related-limit <N>           Maximum related pages in the brief [default: 18]
      --chunk-limit <N>             Maximum retrieved context chunks [default: 10]
      --token-budget <TOKENS>       Token budget across retrieved chunks [default: 1200]
      --max-pages <N>               Maximum distinct source pages in chunk retrieval [default: 8]
      --link-limit <N>              Maximum internal link suggestions [default: 18]
      --category-limit <N>          Maximum category suggestions [default: 8]
      --template-limit <N>          Maximum template summaries [default: 16]
      --docs-profile <PROFILE>      Docs profile to use for bridged authoring retrieval [default: remilia-wiki]
      --contract-profile <PROFILE>  Contract traversal profile: index|author|implementation [default: author] [possible values: index, author, implementation]
      --contract-query <QUERY>      Optional contract traversal query separate from TOPIC, such as "species infobox taxonomy"
      --format <FORMAT>             Output format: text|json [default: json] [possible values: text, json]
      --view <VIEW>                 JSON view: brief|full [default: brief] [possible values: brief, full]
      --intent <INTENT>             Authoring intent: new|expand|audit|refresh [default: new] [possible values: new, expand, audit, refresh]
      --diversify                   Enable lexical chunk de-duplication and diversification
      --no-diversify                Disable lexical chunk de-duplication and diversification
  -h, --help                        Print help
```

## knowledge contracts

```text
Plan and search token-budgeted authoring contracts

Usage: wikitool knowledge contracts [OPTIONS] <COMMAND>

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

## knowledge contracts search

```text
Search the indexed authoring contract graph

Usage: wikitool knowledge contracts search [OPTIONS] <QUERY>

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

## knowledge contracts plan

```text
Plan contract traversal for a topic or draft

Usage: wikitool knowledge contracts plan [OPTIONS] [TOPIC]

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

## knowledge interview

```text
Create, validate, show, and audit knowledge interview briefs

Usage: wikitool knowledge interview [OPTIONS] <COMMAND>

Commands:
  init       Create a timestamped knowledge interview brief and sidecars
  validate   Validate a knowledge interview brief and sidecars
  show       Show a knowledge interview brief summary
  audit      Audit all knowledge interview briefs in the local ledger
  open-item  Append or list structured interview open items
  help       Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## knowledge interview init

```text
Create a timestamped knowledge interview brief and sidecars

Usage: wikitool knowledge interview init [OPTIONS] <TITLE>

Arguments:
  <TITLE>  Article title or topic for the interview

Options:
      --intent <INTENT>               Interview intent: new|expand|audit|refresh [default: new] [possible values: new, expand, audit, refresh]
      --project-root <PATH>
      --agent <AGENT>                 Agent label for brief metadata
      --data-dir <PATH>
      --config <PATH>
      --source-article <TITLE>        Existing article title this interview concerns
      --diagnostics                   Print resolved runtime diagnostics
      --related-draft <PATH>          Related draft path to record in brief metadata
      --timestamp <YYYYMMDDTHHMMSSZ>  UTC ledger timestamp; defaults to current time
      --force                         Overwrite files if the timestamped brief already exists
      --format <FORMAT>               Output format: text|json [default: json] [possible values: text, json]
  -h, --help                          Print help
```

## knowledge interview validate

```text
Validate a knowledge interview brief and sidecars

Usage: wikitool knowledge interview validate [OPTIONS] <PATH>

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

## knowledge interview show

```text
Show a knowledge interview brief summary

Usage: wikitool knowledge interview show [OPTIONS] <PATH>

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

## knowledge interview audit

```text
Audit all knowledge interview briefs in the local ledger

Usage: wikitool knowledge interview audit [OPTIONS]

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

## knowledge interview open-item

```text
Append or list structured interview open items

Usage: wikitool knowledge interview open-item [OPTIONS] <COMMAND>

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

## knowledge interview open-item add

```text
Append a structured open item to an interview brief sidecar

Usage: wikitool knowledge interview open-item add [OPTIONS] --kind <KIND> --text <TEXT> <PATH>

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

## knowledge interview open-item list

```text
List structured open items for an interview brief

Usage: wikitool knowledge interview open-item list [OPTIONS] <PATH>

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

## knowledge interview open-item update

```text
Update an existing open item's status, note, or text

Usage: wikitool knowledge interview open-item update [OPTIONS] --item-id <ID> <PATH>

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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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
      --view <VIEW>            JSON view: brief|full [default: brief] [possible values: brief, full]
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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
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
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research

```text
Inspect target-wiki evidence and fetch source URLs without mutating the wiki

Usage: wikitool research [OPTIONS] <COMMAND>

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

## research wiki-search

```text
Search the configured wiki API for subject evidence

Usage: wikitool research wiki-search [OPTIONS] <QUERY>

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
      --format <FORMAT>        Output format: wikitext|html|rendered-html [default: html] [possible values: wikitext, html, rendered-html]
      --project-root <PATH>
      --data-dir <PATH>
      --output <FORMAT>        Output wrapper: text|json [default: json] [possible values: text, json]
      --config <PATH>
      --refresh                Refresh the research cache entry before returning output
      --diagnostics            Print resolved runtime diagnostics
      --no-cache               Bypass the research cache for this fetch
      --content-limit <CHARS>  Limit returned content characters; cached source content remains complete
      --no-content             Omit fetched content from output while keeping metadata and extract
      --no-discover            Skip machine-surface discovery when a fetch fails
      --discover-limit <N>     Limit machine-surface entries included with failed fetch diagnostics [default: 12]
  -h, --help                   Print help
```

## research archive

```text
Mirror raw web pages and requisites into a local manifest archive

Usage: wikitool research archive [OPTIONS] <URL>

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

## research discover

```text
Discover public machine-readable source surfaces for a URL

Usage: wikitool research discover [OPTIONS] <URL>

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

## research session

```text
Manage human-solved source access sessions

Usage: wikitool research session [OPTIONS] <COMMAND>

Commands:
  import  Import source-issued browser cookies for a domain
  list    List imported research access sessions without cookie values
  show    Show one imported research access session without cookie values
  clear   Clear one imported research access session
  prune   Remove expired research access sessions
  help    Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research session import

```text
Import source-issued browser cookies for a domain

Usage: wikitool research session import [OPTIONS] --cookies <PATH|-|COOKIE_HEADER> <URL>

Arguments:
  <URL>

Options:
      --cookies <PATH|-|COOKIE_HEADER>  Read cookies from Netscape cookies.txt, JSON, stdin (-), or a literal Cookie header
      --project-root <PATH>
      --data-dir <PATH>
      --user-agent <UA>                 Pin the browser user-agent used when the cookies were obtained
      --config <PATH>
      --ttl-seconds <SECONDS>           Expire this local session after the supplied number of seconds
      --diagnostics                     Print resolved runtime diagnostics
      --format <FORMAT>                 Output format: text|json [default: json] [possible values: text, json]
  -h, --help                            Print help
```

## research session list

```text
List imported research access sessions without cookie values

Usage: wikitool research session list [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research session show

```text
Show one imported research access session without cookie values

Usage: wikitool research session show [OPTIONS] <DOMAIN>

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

## research session clear

```text
Clear one imported research access session

Usage: wikitool research session clear [OPTIONS] <DOMAIN>

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

## research session prune

```text
Remove expired research access sessions

Usage: wikitool research session prune [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: json] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## research mediawiki-templates

```text
Inspect live template contracts used by a source MediaWiki page

Usage: wikitool research mediawiki-templates [OPTIONS] <URL>

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
  profile       Show the combined live/profile-aware wiki surface
  rules         Show the structured local editorial rules overlay
  surface       Show the agent-facing template, module, asset, and extension authoring surface
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

## wiki cargo

```text
Query the live wiki's Cargo extension tables

Usage: wikitool wiki cargo [OPTIONS] <COMMAND>

Commands:
  count  Count rows in a live Cargo table
  help   Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
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

## wiki profile

```text
Show the combined live/profile-aware wiki surface

Usage: wikitool wiki profile [OPTIONS] <COMMAND>

Commands:
  sync    Refresh the local rules overlay and live capability snapshot
  show    Show the current combined profile snapshot
  remote  Inspect a remote target wiki capability profile without storing it locally
  help    Print this message or the help of the given subcommand(s)

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

## wiki profile remote

```text
Inspect a remote target wiki capability profile without storing it locally

Usage: wikitool wiki profile remote [OPTIONS] <URL>

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

## wiki rules

```text
Show the structured local editorial rules overlay

Usage: wikitool wiki rules [OPTIONS] <COMMAND>

Commands:
  show  Show the current profile rules overlay
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
Show the current profile rules overlay

Usage: wikitool wiki rules show [OPTIONS]

Options:
      --format <FORMAT>      Output format: text|json [default: text] [possible values: text, json]
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki surface

```text
Show the agent-facing template, module, asset, and extension authoring surface

Usage: wikitool wiki surface [OPTIONS] <COMMAND>

Commands:
  sync  Refresh and show the agent-facing authoring surface
  show  Show the current agent-facing authoring surface
  help  Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## wiki surface sync

```text
Refresh and show the agent-facing authoring surface

Usage: wikitool wiki surface sync [OPTIONS]

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
  -h, --help                        Print help
```

## wiki surface show

```text
Show the current agent-facing authoring surface

Usage: wikitool wiki surface show [OPTIONS]

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
  -h, --help                        Print help
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

## article

```text
Lint and mechanically remediate article drafts

Usage: wikitool article [OPTIONS] <COMMAND>

Commands:
  lint     Lint article wikitext against wiki/profile rules
  fix      Apply safe mechanical fixes to article wikitext
  promote  Copy a reviewed state draft into the sync tree
  help     Print this message or the help of the given subcommand(s)

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
Copy a reviewed state draft into the sync tree

Usage: wikitool article promote [OPTIONS] --title <TITLE> <PATH>

Arguments:
  <PATH>  State-draft path under the canonical .wikitool/drafts/ directory

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

## workflow

```text
First-run setup and session/full runtime refresh workflows

Usage: wikitool workflow [OPTIONS] <COMMAND>

Commands:
  session-refresh  Refresh runtime content and agent authoring context
  full-refresh     Rebuild local runtime from scratch and re-warm knowledge
  help             Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>
      --data-dir <PATH>
      --config <PATH>
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## workflow session-refresh

```text
Refresh runtime content and agent authoring context

Usage: wikitool workflow session-refresh [OPTIONS]

Options:
      --project-root <PATH>
      --templates               Create templates/ during initialization (default: true)
      --data-dir <PATH>
      --no-templates            Do not create templates/ during initialization
      --config <PATH>
      --full                    Perform a full pull instead of an incremental session pull
      --diagnostics             Print resolved runtime diagnostics
      --pull                    Pull content after initialization (default: true)
      --no-pull                 Skip content pull during session refresh
      --docs-profile <PROFILE>  Docs profile to hydrate during knowledge warmup [default: remilia-wiki]
      --docs-mode <MODE>        Docs hydration mode for knowledge warmup: missing|refresh|skip [default: missing] [possible values: missing, refresh, skip]
  -h, --help                    Print help
```

## workflow full-refresh

```text
Rebuild local runtime from scratch and re-warm knowledge

Usage: wikitool workflow full-refresh [OPTIONS]

Options:
      --project-root <PATH>
      --yes                     Assume yes; do not prompt for confirmation
      --data-dir <PATH>
      --templates               Create templates/ during initialization (default: true)
      --config <PATH>
      --no-templates            Do not create templates/ during initialization
      --diagnostics             Print resolved runtime diagnostics
      --docs-profile <PROFILE>  Docs profile to hydrate during knowledge warmup [default: remilia-wiki]
  -h, --help                    Print help
```
