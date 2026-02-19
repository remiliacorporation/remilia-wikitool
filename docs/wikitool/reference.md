# Wikitool Command Reference

This file is generated from Rust CLI help output. Do not edit manually.

Regenerate:

```bash
scripts/generate-wikitool-reference.ps1
scripts/generate-wikitool-reference.sh
```

## Global

```text
Rust rewrite CLI for remilia-wikitool

Usage: wikitool.exe [OPTIONS] [COMMAND]

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
  contracts            Contract bootstrap and differential harness helpers
  help                 Print this message or the help of the given subcommand(s)

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
  -V, --version              Print version
```

## init

```text
Usage: wikitool.exe init [OPTIONS]

Options:
      --project-root <PATH>  
      --templates            Create templates/ during initialization
      --data-dir <PATH>      
      --force                Overwrite existing config/parser files
      --config <PATH>        
      --no-config            Skip writing .wikitool/config.toml
      --diagnostics          Print resolved runtime diagnostics
      --no-parser-config     Skip writing .wikitool/remilia-parser.json
  -h, --help                 Print help
```

## pull

```text
Usage: wikitool.exe pull [OPTIONS]

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
Usage: wikitool.exe push [OPTIONS]

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
Usage: wikitool.exe diff [OPTIONS]

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
Usage: wikitool.exe status [OPTIONS]

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
Usage: wikitool.exe context [OPTIONS] <TITLE>

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
Usage: wikitool.exe search [OPTIONS] <QUERY>

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
Usage: wikitool.exe search-external [OPTIONS] <QUERY>

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
Usage: wikitool.exe validate [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lint

```text
Usage: wikitool.exe lint [OPTIONS] [TITLE]

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
Usage: wikitool.exe fetch [OPTIONS] <URL>

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
Usage: wikitool.exe export [OPTIONS] <URL>

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
Usage: wikitool.exe delete [OPTIONS] --reason <TEXT> <TITLE>

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
Usage: wikitool.exe db [OPTIONS] <COMMAND>

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
Usage: wikitool.exe db stats [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db sync

```text
Usage: wikitool.exe db sync [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## db migrate

```text
Usage: wikitool.exe db migrate [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs

```text
Usage: wikitool.exe docs [OPTIONS] <COMMAND>

Commands:
  import            
  import-technical  
  list              
  update            
  remove            
  search            
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
Usage: wikitool.exe docs import [OPTIONS] [EXTENSION]...

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
Usage: wikitool.exe docs import-technical [OPTIONS] [PAGE]...

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

## docs list

```text
Usage: wikitool.exe docs list [OPTIONS]

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
Usage: wikitool.exe docs update [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## docs remove

```text
Usage: wikitool.exe docs remove [OPTIONS] <TARGET>

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
Usage: wikitool.exe docs search [OPTIONS] <QUERY>

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

## seo inspect

```text
Usage: wikitool.exe seo inspect [OPTIONS] <TARGET>

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

## net inspect

```text
Usage: wikitool.exe net inspect [OPTIONS] <TARGET>

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

## perf lighthouse

```text
Usage: wikitool.exe perf lighthouse [OPTIONS] [TARGET]

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

## import cargo

```text
Usage: wikitool.exe import cargo [OPTIONS] --table <NAME> <PATH>

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
Usage: wikitool.exe index [OPTIONS] <COMMAND>

Commands:
  rebuild           
  stats             
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
Usage: wikitool.exe index rebuild [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index stats

```text
Usage: wikitool.exe index stats [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index backlinks

```text
Usage: wikitool.exe index backlinks [OPTIONS] <TITLE>

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
Usage: wikitool.exe index orphans [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## index prune-categories

```text
Usage: wikitool.exe index prune-categories [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp:generate-config

```text
Usage: wikitool.exe lsp:generate-config [OPTIONS]

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
Usage: wikitool.exe lsp:status [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## lsp:info

```text
Usage: wikitool.exe lsp:info [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

## contracts

```text
Contract bootstrap and differential harness helpers

Usage: wikitool.exe contracts [OPTIONS] <COMMAND>

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

Usage: wikitool.exe contracts snapshot [OPTIONS]

Options:
      --project-root <PROJECT_ROOT>    [default: .]
      --content-dir <CONTENT_DIR>      [default: wiki_content]
      --data-dir <PATH>                
      --config <PATH>                  
      --templates-dir <TEMPLATES_DIR>  [default: custom/templates]
      --diagnostics                    Print resolved runtime diagnostics
  -h, --help                           Print help
```

## contracts command-surface

```text
Print frozen command-surface contract as JSON

Usage: wikitool.exe contracts command-surface [OPTIONS]

Options:
      --project-root <PATH>  
      --data-dir <PATH>      
      --config <PATH>        
      --diagnostics          Print resolved runtime diagnostics
  -h, --help                 Print help
```

