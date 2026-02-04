/**
 * Phase 1 Integration Test
 *
 * Verifies the core library can:
 * 1. Initialize the SQLite database
 * 2. Create the schema
 * 3. Store and retrieve pages
 * 4. Handle basic operations
 *
 * Run with: bun tests/phase1-test.js
 */

import { createDatabase } from '../packages/core/dist/storage/sqlite.js';
import { createFilesystem } from '../packages/core/dist/storage/filesystem.js';
import { Namespace, titleToFilepath, parseRedirect } from '../packages/core/dist/models/namespace.js';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DB_PATH = ':memory:';

async function runTests() {
  console.log('='.repeat(60));
  console.log('Phase 1 Integration Test');
  console.log('='.repeat(60));

  let passed = 0;
  let failed = 0;

  // Test 1: Database initialization
  console.log('\n1. Testing database initialization...');
  try {
    const db = await createDatabase(DB_PATH);
    console.log('   ✓ Database created and initialized');
    passed++;

    // Test 2: Config operations
    console.log('\n2. Testing config operations...');
    const schemaVersion = db.getConfig('schema_version');
    const expectedVersion = db.getExpectedVersion();
    if (schemaVersion === expectedVersion) {
      console.log('   ✓ Schema version is correct: ' + schemaVersion);
      passed++;
    } else {
      console.log('   ✗ Schema version incorrect: ' + schemaVersion);
      failed++;
    }

    db.setConfig('test_key', 'test_value');
    const testValue = db.getConfig('test_key');
    if (testValue === 'test_value') {
      console.log('   ✓ Config set/get works');
      passed++;
    } else {
      console.log('   ✗ Config set/get failed');
      failed++;
    }

    // Test 3: Page operations
    console.log('\n3. Testing page operations...');
    const pageId = db.upsertPage({
      title: 'Test Page',
      namespace: Namespace.Main,
      page_type: 'article',
      filename: 'Test_Page.wiki',
      filepath: 'wiki_content/Main/Test_Page.wiki',
      content: 'This is a test page.',
      content_hash: 'abc123',
      sync_status: 'synced',
    });

    if (pageId > 0) {
      console.log('   ✓ Page inserted with ID: ' + pageId);
      passed++;
    } else {
      console.log('   ✗ Page insert failed');
      failed++;
    }

    const page = db.getPage('Test Page');
    if (page && page.title === 'Test Page') {
      console.log('   ✓ Page retrieved successfully');
      passed++;
    } else {
      console.log('   ✗ Page retrieval failed');
      failed++;
    }

    // Test 4: Update page
    db.upsertPage({
      title: 'Test Page',
      content: 'Updated content',
      content_hash: 'def456',
    });
    const updated = db.getPage('Test Page');
    if (updated?.content === 'Updated content') {
      console.log('   ✓ Page update works');
      passed++;
    } else {
      console.log('   ✗ Page update failed');
      failed++;
    }

    // Test 5: Page counts
    console.log('\n4. Testing page counts...');
    const counts = db.getPageCounts();
    if (counts.synced === 1) {
      console.log('   ✓ Page counts correct: ' + JSON.stringify(counts));
      passed++;
    } else {
      console.log('   ✗ Page counts incorrect: ' + JSON.stringify(counts));
      failed++;
    }

    // Test 6: Sync log
    console.log('\n5. Testing sync log...');
    db.logSync({
      operation: 'pull',
      page_title: 'Test Page',
      status: 'success',
      revision_id: 123,
      error_message: null,
      details: '{"test": true}',
    });
    const logs = db.getSyncLogs(10);
    if (logs.length > 0 && logs[0].operation === 'pull') {
      console.log('   ✓ Sync log works');
      passed++;
    } else {
      console.log('   ✗ Sync log failed');
      failed++;
    }

    // Test 7: FTS indexing
    console.log('\n6. Testing full-text search...');
    db.indexPage('content', 'Test Page', 'This is some searchable content about wikis');
    const searchResults = db.searchFts('searchable');
    if (searchResults.length > 0) {
      console.log('   ✓ FTS indexing and search works');
      passed++;
    } else {
      console.log('   ✗ FTS failed');
      failed++;
    }

    // Test 8: Namespace utilities
    console.log('\n7. Testing namespace utilities...');
    const filepath = titleToFilepath('Template:Infobox person', false, 'wiki_content', 'custom/templates');
    if (filepath.includes('infobox')) {
      console.log('   ✓ Template categorization works: ' + filepath);
      passed++;
    } else {
      console.log('   ✗ Template categorization failed: ' + filepath);
      failed++;
    }

    // Test 9: Redirect parsing
    console.log('\n8. Testing redirect parsing...');
    const [isRedirect, target] = parseRedirect('#REDIRECT [[Main Page]]');
    if (isRedirect && target === 'Main Page') {
      console.log('   ✓ Redirect parsing works');
      passed++;
    } else {
      console.log('   ✗ Redirect parsing failed');
      failed++;
    }

    // Test 10: Database stats
    console.log('\n9. Testing database stats...');
    const stats = db.getStats();
    if (stats.totalPages === 1) {
      console.log('   ✓ Stats work: ' + JSON.stringify(stats));
      passed++;
    } else {
      console.log('   ✗ Stats failed: ' + JSON.stringify(stats));
      failed++;
    }

    db.close();
    console.log('   ✓ Database closed');

  } catch (error) {
    console.log('   ✗ Error: ' + error.message);
    console.log(error.stack);
    failed++;
  }

  // Summary
  console.log('\n' + '='.repeat(60));
  console.log(`Results: ${passed} passed, ${failed} failed`);
  console.log('='.repeat(60));

  process.exit(failed > 0 ? 1 : 0);
}

runTests();
