/**
 * Phase 0: Test wikilint with proper configuration
 */

const sampleWikitext = `{{SHORTDESC:Generative PFP NFT collection}}
{{Infobox NFT collection
|name = Milady Maker
|image = Milady.jpg
}}

'''Milady Maker''' is a collection of NFTs by [[Remilia Corporation]].<ref>{{Cite web|url=https://example.com|title=Example}}</ref>

==Origins==

Content here with [[internal link]] and {{template|param=value}}.

==References==
{{Reflist}}

[[Category:NFT Collections]]
`;

async function main() {
  console.log('Testing wikilint with configuration...\n');

  const wikilint = await import('wikilint');
  const { lintConfig, parse, fetchConfig } = wikilint.default;

  // Check lintConfig
  console.log('--- lintConfig ---');
  console.log('Type:', typeof lintConfig);
  if (lintConfig) {
    console.log('Keys:', Object.keys(lintConfig));
  }

  // Try to create a config
  console.log('\n--- Creating config ---');

  // Check if there's a way to create config manually
  const Parser = wikilint.default.Parser || wikilint.Parser;
  console.log('Parser available:', !!Parser);

  // Try fetchConfig
  console.log('\n--- fetchConfig ---');
  console.log('fetchConfig type:', typeof fetchConfig);

  // Try with a default/empty config approach
  console.log('\n--- Alternative: Direct wikiparser import ---');

  // wikilint uses wikiparser-node internally - let's check if we can access it
  try {
    // Check node_modules for the parser
    const wikiparser = await import('wikiparser-node');
    console.log('wikiparser-node available:', !!wikiparser);
    console.log('wikiparser exports:', Object.keys(wikiparser.default || wikiparser));

    const Parser = wikiparser.default || wikiparser.Parser || wikiparser;
    if (typeof Parser === 'function' || Parser.parse) {
      console.log('\n--- Parsing with wikiparser-node ---');
      const parseFunc = Parser.parse || Parser;
      const ast = typeof parseFunc === 'function' ? parseFunc(sampleWikitext) : null;

      if (ast) {
        console.log('AST type:', ast.constructor?.name);
        console.log('AST has:', Object.keys(ast).slice(0, 10));

        // Try to get tokens/nodes
        if (ast.querySelectorAll) {
          const templates = ast.querySelectorAll('template');
          console.log('Templates found:', templates?.length);
        }
        if (ast.childNodes) {
          console.log('Child nodes:', ast.childNodes.length);
        }
      }
    }
  } catch (err) {
    console.log('wikiparser-node not directly accessible:', err.message);
  }

  // Summary
  console.log('\n' + '='.repeat(50));
  console.log('Summary');
  console.log('='.repeat(50));
  console.log(`
wikilint requires configuration from a MediaWiki site to work properly.
For our validation use case, we have two options:

1. Use wikilint with fetchConfig() pointing to wiki.remilia.org
   - Pros: Full lint rules, accurate parsing
   - Cons: Requires network, site-specific config

2. Use simpler validation in @wikitool/core
   - Check for required elements (SHORTDESC, categories)
   - Validate template syntax with regex
   - Leave full linting to the wiki's built-in parser

Recommendation: Option 2 for Phase 1-4, add wikilint integration in Phase 5
when we have the LSP and can properly configure it.
`);
}

main().catch(console.error);
