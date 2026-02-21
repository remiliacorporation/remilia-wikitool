# Wikitool Command Reference

This file is generated from Rust CLI help output. Do not edit manually.

Regenerate:

```bash
wikitool docs generate-reference
```

## Global

```text
Wiki management CLI

Usage: wikitool [OPTIONS] [COMMAND]

Commands:
  init                 
  pull                 
  push                 
  diff                 
  status               
  context              
  search               
  search-external      
  validate             
  lint                 
  fetch                
  export               
  delete               
  db                   
  docs                 
  seo                  
  net                  
  perf                 
  import               
  index                
  lsp:generate-config  
  lsp:status           
  lsp:info             
  workflow             
  release              
  dev                  
  contracts            Contract bootstrap and differential harness helpers
  help                 Print this message or the help of the given subcommand(s)

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
  -h, --help                 Print help
```

## push

```text
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
  -h, --help                 Print help
```

## diff

```text
Usage: wikitool diff [OPTIONS]

Options:
      --project-root <PATH>  
      --templates            Include template/module/mediawiki namespaces
      --data-dir <PATH>      
      --verbose              Show hash-level details for modified entries
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## status

```text
Usage: wikitool status [OPTIONS]

Options:
      --modified             Only show modified
      --project-root <PATH>  
      --conflicts            Only show conflicts
      --data-dir <PATH>      
      --config <PATH>        
      --templates            Include templates
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## context

```text
Usage: wikitool context [OPTIONS] <TITLE>

Arguments:
  <TITLE>  

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## search

```text
Usage: wikitool search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## search-external

```text
Usage: wikitool search-external [OPTIONS] <QUERY>

Arguments:
  <QUERY>  

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## validate

```text
Usage: wikitool validate [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lint

```text
Usage: wikitool lint [OPTIONS] [TITLE]

Arguments:
  [TITLE]  

Options:
      --format <FORMAT>      Output format: text|json [default: text]
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
Usage: wikitool fetch [OPTIONS] <URL>

Arguments:
  <URL>  

Options:
      --format <FORMAT>      Output format: wikitext|html [default: wikitext]
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
Usage: wikitool db [OPTIONS] <COMMAND>

Commands:
  stats    
  sync     
  migrate  
  help     Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db stats

```text
Usage: wikitool db stats [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db sync

```text
Usage: wikitool db sync [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db migrate

```text
Usage: wikitool db migrate [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs

```text
Usage: wikitool docs [OPTIONS] <COMMAND>

Commands:
  import              
  import-technical    
  generate-reference  
  list                
  update              
  remove              
  search              
  help                Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs import

```text
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
Usage: wikitool docs import-technical [OPTIONS] [PAGE]...

Arguments:
  [PAGE]...  

Options:
      --project-root <PATH>  
      --subpages             Include subpages for selected pages/types
      --data-dir <PATH>      
      --hooks                Import all hook documentation
      --config               Import configuration variable docs
      --api                  Import API documentation
      --diagnostics          Print resolved runtime diagnostics
  -l, --limit <LIMIT>        Limit subpage imports per task [default: 100]
  -h, --help                 Print help
```

## docs generate-reference

```text
Usage: wikitool docs generate-reference [OPTIONS]

Options:
      --output <PATH>        Output markdown path (default: docs/wikitool/reference.md in current directory)
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs list

```text
Usage: wikitool docs list [OPTIONS]

Options:
      --outdated             Show only outdated docs
      --project-root <PATH>  
      --data-dir <PATH>      
      --type <TYPE>          Filter technical docs by type
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs update

```text
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
Usage: wikitool docs search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  

Options:
      --project-root <PATH>  
      --tier <TIER>          Search tier (extension, technical)
      --data-dir <PATH>      
  -l, --limit <LIMIT>        Limit result count [default: 20]
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## seo

```text
Usage: wikitool seo [OPTIONS] <COMMAND>

Commands:
  inspect  
  help     Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## seo inspect

```text
Usage: wikitool seo inspect [OPTIONS] <TARGET>

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
Usage: wikitool net [OPTIONS] <COMMAND>

Commands:
  inspect  
  help     Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## net inspect

```text
Usage: wikitool net inspect [OPTIONS] <TARGET>

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

## perf

```text
Usage: wikitool perf [OPTIONS] <COMMAND>

Commands:
  lighthouse  
  help        Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## perf lighthouse

```text
Usage: wikitool perf lighthouse [OPTIONS] [TARGET]

Arguments:
  [TARGET]  

Options:
      --output <FORMAT>       Output format: html|json [default: html]
      --project-root <PATH>   
      --data-dir <PATH>       
      --out <PATH>            Report output path
      --categories <LIST>     Comma-separated categories
      --config <PATH>         
      --chrome-flags <FLAGS>  Pass Chrome flags to Lighthouse
      --diagnostics           Print resolved runtime diagnostics
      --show-version          Print resolved Lighthouse binary + version and exit
      --json                  Output JSON summary
      --no-meta               Omit metadata from JSON output
      --url <URL>             Override target URL
  -h, --help                  Print help
```

## import

```text
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
      --format <FORMAT>        Output format: text|json [default: text]
      --article-header         Add SHORTDESC + Article quality header in main namespace
      --no-meta                Omit metadata from JSON output
  -h, --help                   Print help
```

## index

```text
Usage: wikitool index [OPTIONS] <COMMAND>

Commands:
  rebuild           Rebuild the local search index from wiki_content and templates
  stats             Show index statistics
  chunks            Retrieve token-budgeted content chunks from indexed pages
  backlinks         
  orphans           
  prune-categories  
  help              Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index rebuild

```text
Rebuild the local search index from wiki_content and templates

Usage: wikitool index rebuild [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index stats

```text
Show index statistics

Usage: wikitool index stats [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index chunks

```text
Retrieve token-budgeted content chunks from indexed pages

Usage: wikitool index chunks [OPTIONS] [TITLE]

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
      --format <FORMAT>        Output format: text|json [default: text]
      --diversify              Enable lexical de-duplication and diversification
      --no-diversify           Disable lexical de-duplication and diversification
  -h, --help                   Print help
```

## index backlinks

```text
Usage: wikitool index backlinks [OPTIONS] <TITLE>

Arguments:
  <TITLE>  

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index orphans

```text
Usage: wikitool index orphans [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index prune-categories

```text
Usage: wikitool index prune-categories [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp:generate-config

```text
Usage: wikitool lsp:generate-config [OPTIONS]

Options:
      --force                Overwrite parser config if it already exists
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp:status

```text
Usage: wikitool lsp:status [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp:info

```text
Usage: wikitool lsp:info [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## workflow

```text
Usage: wikitool workflow [OPTIONS] <COMMAND>

Commands:
  bootstrap       
  full-refresh    
  authoring-pack  Generate a token-budgeted knowledge pack for article authoring
  help            Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## workflow bootstrap

```text
Usage: wikitool workflow bootstrap [OPTIONS]

Options:
      --project-root <PATH>  
      --templates            Create templates/ during initialization (default: true)
      --data-dir <PATH>      
      --no-templates         Do not create templates/ during initialization
      --config <PATH>        
      --pull                 Pull content after initialization (default: true)
      --diagnostics          Print resolved runtime diagnostics
      --no-pull              Skip content pull after initialization
      --skip-reference       Skip docs reference generation
      --skip-git-hooks       Skip commit-msg hook installation
  -h, --help                 Print help
```

## workflow full-refresh

```text
Usage: wikitool workflow full-refresh [OPTIONS]

Options:
      --project-root <PATH>  
      --yes                  Assume yes; do not prompt for confirmation
      --data-dir <PATH>      
      --templates            Create templates/ during initialization (default: true)
      --config <PATH>        
      --no-templates         Do not create templates/ during initialization
      --diagnostics          Print resolved runtime diagnostics
      --skip-reference       Skip docs reference generation
  -h, --help                 Print help
```

## workflow authoring-pack

```text
Generate a token-budgeted knowledge pack for article authoring

Usage: wikitool workflow authoring-pack [OPTIONS] [TOPIC]

Arguments:
  [TOPIC]  Primary article topic/title for retrieval

Options:
      --project-root <PATH>    
      --stub-path <PATH>       Optional stub wikitext file used for link/template hint extraction
      --data-dir <PATH>        
      --related-limit <N>      Maximum related pages in the pack [default: 18]
      --chunk-limit <N>        Maximum retrieved context chunks [default: 10]
      --config <PATH>          
      --diagnostics            Print resolved runtime diagnostics
      --token-budget <TOKENS>  Token budget across retrieved chunks [default: 1200]
      --max-pages <N>          Maximum distinct source pages in chunk retrieval [default: 8]
      --link-limit <N>         Maximum internal link suggestions [default: 18]
      --category-limit <N>     Maximum category suggestions [default: 8]
      --template-limit <N>     Maximum template summaries [default: 16]
      --format <FORMAT>        Output format: text|json [default: json]
      --diversify              Enable lexical chunk de-duplication and diversification
      --no-diversify           Disable lexical chunk de-duplication and diversification
  -h, --help                   Print help
```

## release

```text
Usage: wikitool release [OPTIONS] <COMMAND>

Commands:
  build-ai-pack  
  package        
  build-matrix   
  help           Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## release build-ai-pack

```text
Usage: wikitool release build-ai-pack [OPTIONS]

Options:
      --project-root <PATH>       
      --repo-root <PATH>          Wikitool repository root (default: current directory)
      --data-dir <PATH>           
      --output-dir <PATH>         Output directory (default: <repo>/dist/ai-pack)
      --config <PATH>             
      --host-project-root <PATH>  Optional host project root containing CLAUDE.md + .claude/{rules,skills}
      --diagnostics               Print resolved runtime diagnostics
  -h, --help                      Print help
```

## release package

```text
Usage: wikitool release package [OPTIONS]

Options:
      --project-root <PATH>       
      --repo-root <PATH>          Wikitool repository root (default: current directory)
      --binary-path <PATH>        Release binary path (default: <repo>/target/release/wikitool[.exe])
      --data-dir <PATH>           
      --config <PATH>             
      --output-dir <PATH>         Output directory (default: <repo>/dist/release)
      --diagnostics               Print resolved runtime diagnostics
      --host-project-root <PATH>  Optional host project root containing CLAUDE.md + .claude/{rules,skills}
  -h, --help                      Print help
```

## release build-matrix

```text
Usage: wikitool release build-matrix [OPTIONS]

Options:
      --project-root <PATH>       
      --repo-root <PATH>          Wikitool repository root (default: current directory)
      --data-dir <PATH>           
      --targets <TRIPLE>          Target triples to build (repeat or use comma-separated list). Defaults to windows/linux/macos x86_64 targets.
      --config <PATH>             
      --output-dir <PATH>         Output directory for staged folders and zip artifacts (default: <repo>/dist/release-matrix)
      --artifact-version <LABEL>  Version label used in bundle names (default: v<CARGO_PKG_VERSION>)
      --diagnostics               Print resolved runtime diagnostics
      --unversioned-names         Use unversioned bundle names (wikitool-<target>) for CI/ephemeral artifacts
      --cargo-bin <PATH>          Cargo executable path used for target builds (default: cargo)
      --skip-build                Skip cargo build and package existing target binaries
      --locked                    Use cargo --locked for target builds (default: true)
      --no-locked                 Do not pass --locked to cargo builds
      --host-project-root <PATH>  Optional host project root containing CLAUDE.md + .claude/{rules,skills}
  -h, --help                      Print help
```

## dev

```text
Usage: wikitool dev [OPTIONS] <COMMAND>

Commands:
  install-git-hooks  
  help               Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## dev install-git-hooks

```text
Usage: wikitool dev install-git-hooks [OPTIONS]

Options:
      --project-root <PATH>  
      --repo-root <PATH>     Repository root containing .git/hooks (default: current directory)
      --data-dir <PATH>      
      --source <PATH>        Hook source file (default: scripts/git-hooks/commit-msg under repo root)
      --allow-missing-git    Do not fail when .git/hooks is missing (useful for zip-distributed binaries)
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## contracts

```text
Contract bootstrap and differential harness helpers

Usage: wikitool contracts [OPTIONS] <COMMAND>

Commands:
  snapshot         Generate an offline fixture snapshot used by the differential harness
  command-surface  Print frozen command-surface contract as JSON
  help             Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## contracts snapshot

```text
Generate an offline fixture snapshot used by the differential harness

Usage: wikitool contracts snapshot [OPTIONS]

Options:
      --project-root <PROJECT_ROOT>    [default: .]
      --content-dir <CONTENT_DIR>      [default: wiki_content]
      --data-dir <PATH>                
      --config <PATH>                  
      --templates-dir <TEMPLATES_DIR>  [default: templates]
      --diagnostics                    Print resolved runtime diagnostics
  -h, --help                           Print help
```

## contracts command-surface

```text
Print frozen command-surface contract as JSON

Usage: wikitool contracts command-surface [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```
