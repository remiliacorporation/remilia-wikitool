/**
 * Phase 0: Test wikilint's parse function
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
  console.log('Testing wikilint parse function...\n');

  const wikilint = await import('wikilint');
  const { parse, normalizeTitle, createLanguageService } = wikilint.default;

  console.log('Available functions:', Object.keys(wikilint.default).join(', '));

  // Test normalizeTitle
  console.log('\n--- normalizeTitle ---');
  console.log('normalizeTitle("milady maker"):', normalizeTitle('milady maker'));
  console.log('normalizeTitle("Category:NFT_Collections"):', normalizeTitle('Category:NFT_Collections'));

  // Test parse
  console.log('\n--- parse ---');
  try {
    const ast = parse(sampleWikitext);
    console.log('Parse result type:', typeof ast);
    console.log('Parse result constructor:', ast?.constructor?.name);

    if (ast) {
      // Explore AST structure
      console.log('\nAST keys:', Object.keys(ast));

      if (ast.type) console.log('Root type:', ast.type);
      if (ast.childNodes) console.log('Child nodes:', ast.childNodes?.length);
      if (ast.children) console.log('Children:', ast.children?.length);

      // Try to find templates
      console.log('\n--- Finding templates in AST ---');
      function findNodes(node, type, results = []) {
        if (!node) return results;
        if (node.type === type || node.name === type) {
          results.push(node);
        }
        const children = node.childNodes || node.children || [];
        for (const child of children) {
          findNodes(child, type, results);
        }
        return results;
      }

      // Try different type names
      const typeNames = ['template', 'Template', 'TEMPLATE', 'transclusion'];
      for (const typeName of typeNames) {
        const templates = findNodes(ast, typeName);
        if (templates.length > 0) {
          console.log(`Found ${templates.length} nodes of type "${typeName}"`);
          console.log('First template:', JSON.stringify(templates[0], null, 2).slice(0, 500));
          break;
        }
      }

      // Print first level of AST
      console.log('\n--- AST First Level ---');
      const firstLevel = ast.childNodes || ast.children || [];
      for (let i = 0; i < Math.min(5, firstLevel.length); i++) {
        const node = firstLevel[i];
        console.log(`[${i}] type: ${node.type}, name: ${node.name || 'N/A'}`);
      }
    }
  } catch (err) {
    console.log('Parse error:', err.message);
    console.log('Stack:', err.stack);
  }

  // Test createLanguageService
  console.log('\n--- createLanguageService ---');
  try {
    const service = createLanguageService();
    console.log('Language service created:', typeof service);
    console.log('Service methods:', Object.keys(service).slice(0, 10));
  } catch (err) {
    console.log('createLanguageService error:', err.message);
  }
}

main().catch(console.error);
