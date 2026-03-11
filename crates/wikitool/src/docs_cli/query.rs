use super::*;

#[derive(Debug, Args)]
pub(super) struct DocsSearchArgs {
    query: String,
    #[arg(
        long,
        value_name = "TIER",
        help = "Search tier: page|section|symbol|example|extension|technical|profile"
    )]
    tier: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict search to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "text", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 20, help = "Limit result count")]
    limit: usize,
}

#[derive(Debug, Args)]
pub(super) struct DocsContextArgs {
    query: String,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict context retrieval to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "json", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 6, help = "Limit hits per tier")]
    limit: usize,
    #[arg(
        long,
        default_value_t = 1600,
        help = "Approximate token budget for returned context"
    )]
    token_budget: usize,
}

#[derive(Debug, Args)]
pub(super) struct DocsSymbolsArgs {
    query: String,
    #[arg(long, value_name = "KIND", help = "Symbol kind filter")]
    kind: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict symbol lookup to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "text", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 20, help = "Limit result count")]
    limit: usize,
}

pub(super) fn run_docs_search(runtime: &RuntimeOptions, args: DocsSearchArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let hits = search_docs(
        &paths,
        &args.query,
        &DocsSearchOptions {
            tier: args.tier.clone(),
            profile: args.profile.clone(),
            limit: args.limit.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    println!("docs search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {}", collapse_whitespace(&args.query));
    println!("tier: {}", args.tier.as_deref().unwrap_or("<all>"));
    println!("profile: {}", args.profile.as_deref().unwrap_or("<all>"));
    println!("limit: {}", args.limit.max(1));
    println!("hits.count: {}", hits.len());
    for hit in &hits {
        println!(
            "hit: [{}] {} page={} weight={}",
            hit.tier, hit.title, hit.page_title, hit.retrieval_weight
        );
        println!("hit.snippet: {}", hit.snippet);
    }
    Ok(())
}

pub(super) fn run_docs_context(runtime: &RuntimeOptions, args: DocsContextArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let report = build_docs_context(
        &paths,
        &args.query,
        &DocsContextOptions {
            profile: args.profile.clone(),
            limit: args.limit.max(1),
            token_budget: args.token_budget.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("docs context");
    println!("query: {}", report.query);
    println!("profile: {}", report.profile.as_deref().unwrap_or("<all>"));
    println!("token_estimate: {}", report.token_estimate);
    println!("pages.count: {}", report.pages.len());
    for page in &report.pages {
        println!("page: {} weight={}", page.title, page.retrieval_weight);
        println!("page.snippet: {}", page.snippet);
    }
    println!("sections.count: {}", report.sections.len());
    for section in &report.sections {
        println!(
            "section: page={} heading={} weight={}",
            section.page_title,
            section.section_heading.as_deref().unwrap_or("<lead>"),
            section.retrieval_weight
        );
        println!("section.summary: {}", section.summary_text);
    }
    println!("symbols.count: {}", report.symbols.len());
    for symbol in &report.symbols {
        println!(
            "symbol: [{}] {} page={} weight={}",
            symbol.symbol_kind, symbol.symbol_name, symbol.page_title, symbol.retrieval_weight
        );
        println!("symbol.summary: {}", symbol.summary_text);
    }
    println!("examples.count: {}", report.examples.len());
    for example in &report.examples {
        println!(
            "example: [{}] page={} lang={} weight={}",
            example.example_kind,
            example.page_title,
            example.language_hint,
            example.retrieval_weight
        );
        println!("example.summary: {}", example.summary_text);
    }
    Ok(())
}

pub(super) fn run_docs_symbols(runtime: &RuntimeOptions, args: DocsSymbolsArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let hits = lookup_docs_symbols(
        &paths,
        &args.query,
        &DocsSymbolLookupOptions {
            kind: args.kind.clone(),
            profile: args.profile.clone(),
            limit: args.limit.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    println!("docs symbols");
    println!("query: {}", collapse_whitespace(&args.query));
    println!("kind: {}", args.kind.as_deref().unwrap_or("<all>"));
    println!("profile: {}", args.profile.as_deref().unwrap_or("<all>"));
    println!("hits.count: {}", hits.len());
    for hit in &hits {
        println!(
            "symbol: [{}] {} page={} weight={}",
            hit.symbol_kind, hit.symbol_name, hit.page_title, hit.retrieval_weight
        );
        println!("symbol.summary: {}", hit.summary_text);
        if !hit.signature_text.is_empty() {
            println!("symbol.signature: {}", hit.signature_text);
        }
    }
    Ok(())
}
