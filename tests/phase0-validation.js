/**
 * Phase 0: Dependency Validation Tests
 * Run with: bun tests/phase0-validation.js
 */

console.log('='.repeat(60));
console.log('Phase 0: Dependency Validation');
console.log('='.repeat(60));

const results = {
  'bun:sqlite': { status: 'pending', notes: '' },
  'wikitext-lsp': { status: 'pending', notes: '' },
  'wikilint': { status: 'pending', notes: '' },
};

// Test 1: bun:sqlite
console.log('\n[1/3] Testing bun:sqlite...');
try {
  const { Database } = await import('bun:sqlite');
  const db = new Database(':memory:');
  db.run('CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)');
  db.prepare('INSERT INTO test (name) VALUES (?)').run('hello');
  const row = db.prepare('SELECT * FROM test').get();
  if (row && row.name === 'hello') {
    results['bun:sqlite'] = { status: 'OK', notes: 'Built-in driver works' };
    console.log('  ✓ bun:sqlite works');
  }
  db.close();
} catch (err) {
  results['bun:sqlite'] = { status: 'FAIL', notes: err.message };
  console.log('  ✗ bun:sqlite failed:', err.message);
}

// Test 2: wikitext-lsp (optional)
console.log('\n[2/3] Testing wikitext-lsp (optional)...');
try {
  const wikitextLsp = await import('wikitext-lsp');
  const exports = Object.keys(wikitextLsp);
  results['wikitext-lsp'] = { status: 'OK', notes: `Exports: ${exports.join(', ')}` };
  console.log('  ✓ wikitext-lsp loads successfully');
  console.log('    Exports:', exports.slice(0, 5).join(', '), exports.length > 5 ? '...' : '');
} catch (err) {
  results['wikitext-lsp'] = { status: 'SKIP', notes: 'Not installed (optional)' };
  console.log('  - wikitext-lsp not installed (optional)');
}

// Test 3: wikilint (optional)
console.log('\n[3/3] Testing wikilint (optional)...');
try {
  const wikilint = await import('wikilint');
  results['wikilint'] = { status: 'OK', notes: 'Module loads' };
  console.log('  ✓ wikilint loads successfully');
} catch (err) {
  results['wikilint'] = { status: 'SKIP', notes: 'Not installed (optional)' };
  console.log('  - wikilint not installed (optional)');
}

// Summary
console.log('\n' + '='.repeat(60));
console.log('Summary');
console.log('='.repeat(60));
console.log('\nDependency Status:');
for (const [dep, result] of Object.entries(results)) {
  const icon = result.status === 'OK' ? '✓' : result.status === 'SKIP' ? '-' : '✗';
  console.log(`  ${icon} ${dep}: ${result.status}`);
  if (result.notes) {
    console.log(`      ${result.notes}`);
  }
}

// Decision points
console.log('\n' + '='.repeat(60));
console.log('Decision Points');
console.log('='.repeat(60));

if (results['bun:sqlite'].status === 'OK') {
  console.log('\n✓ SQLite: Use bun:sqlite (built-in)');
} else {
  console.log('\n✗ SQLite: CRITICAL - bun:sqlite not available!');
}

if (results['wikitext-lsp'].status === 'OK') {
  console.log('✓ LSP: wikitext-lsp available for editor integration');
} else {
  console.log('! LSP: Reduced to syntax highlighting only (optional)');
}

if (results['wikilint'].status !== 'OK') {
  console.log('- Validation: Use custom validation (wikilint not available)');
}

console.log('\n');
