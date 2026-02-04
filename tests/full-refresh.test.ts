import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { cpSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import {
  MediaWikiClient,
  createDatabase,
  createFilesystem,
  createSyncEngine,
  rebuildIndex,
  getIndexStats,
} from "../packages/core/src/index.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixtureRoot = join(__dirname, "fixtures", "full-refresh");

describe("wikitool full refresh (local files)", () => {
  let rootDir: string;
  let tempRoot: string;
  let dbPath: string;
  let db: Awaited<ReturnType<typeof createDatabase>>;
  let fs: ReturnType<typeof createFilesystem>;
  let engine: ReturnType<typeof createSyncEngine>;
  let initResult: { added: number; errors: string[] };
  let contentFilesCount = 0;
  let templateFilesCount = 0;

  beforeAll(async () => {
    tempRoot = mkdtempSync(join(tmpdir(), "wikitool-full-refresh-"));
    cpSync(fixtureRoot, tempRoot, { recursive: true });
    rootDir = tempRoot;
    dbPath = ":memory:";

    db = await createDatabase(dbPath);
    fs = createFilesystem(rootDir);
    const client = new MediaWikiClient({
      apiUrl: process.env.WIKI_API_URL || "https://wiki.remilia.org/api.php",
    });
    engine = createSyncEngine(db, fs, client);

    const contentFiles = fs.scanContentFiles();
    const templateFiles = fs.scanTemplateFiles();
    contentFilesCount = contentFiles.length;
    templateFilesCount = templateFiles.length;

    expect(contentFiles.some((file) => file.title === "Alpha")).toBe(true);
    expect(templateFiles.some((file) => file.title === "Template:Infobox/style.css")).toBe(true);
    expect(templateFiles.some((file) => file.title === "Module:Navbar/configuration")).toBe(true);

    initResult = await engine.initFromFiles({ includeTemplates: true });
  });

  afterAll(async () => {
    db?.close();
    if (!dbPath || dbPath === ":memory:") {
      if (tempRoot && existsSync(tempRoot)) {
        rmSync(tempRoot, { recursive: true, force: true });
      }
      return;
    }
    if (existsSync(dbPath)) {
      rmSync(dbPath);
    }
    if (tempRoot && existsSync(tempRoot)) {
      rmSync(tempRoot, { recursive: true, force: true });
    }
  });

  test("initializes DB from local content/templates", async () => {
    const totalFiles = contentFilesCount + templateFilesCount;

    expect(contentFilesCount).toBeGreaterThan(0);
    expect(templateFilesCount).toBeGreaterThan(0);

    expect(initResult.errors).toHaveLength(0);
    expect(initResult.added).toBeGreaterThan(0);
    expect(initResult.added).toBeLessThanOrEqual(totalFiles);

    const stats = db.getStats();
    expect(stats.totalPages).toBe(initResult.added);

    const schema = db.validateSchema();
    expect(schema.valid).toBe(true);
  });

  test("rebuilds link index without errors", () => {
    const result = rebuildIndex(db);
    expect(result.errors).toHaveLength(0);
    expect(result.pagesProcessed).toBeGreaterThan(0);

    const stats = getIndexStats(db);
    expect(stats.totalLinks).toBeGreaterThanOrEqual(0);
    expect(stats.totalTemplateUsages).toBeGreaterThanOrEqual(0);
    const dbStats = db.getStats();
    expect(dbStats.totalSections).toBeGreaterThan(0);
  });
});
