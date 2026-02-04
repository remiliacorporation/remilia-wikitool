import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  createDatabase,
  getContextBundle,
  updatePageIndex,
} from "../packages/core/src/index.ts";

describe("incremental index updates", () => {
  let db: Awaited<ReturnType<typeof createDatabase>>;

  beforeAll(async () => {
    db = await createDatabase(":memory:");
  });

  afterAll(() => {
    db?.close();
  });

  test("updates context tables for a single page", () => {
    const content = [
      "{{Infobox person|name=Alice|birth=2000}}",
      "{{SHORTDESC:Test subject}}",
      "Intro with [[Bob]] and [[Category:People]].",
      "== History ==",
      "More about [[Bob]].",
    ].join("\n");

    const pageId = db.upsertPage({
      title: "Alice",
      namespace: 0,
      page_type: "article",
      filename: "Alice.wiki",
      filepath: "wiki_content/Main/Alice.wiki",
      content,
      content_hash: "testhash",
      sync_status: "synced",
    });

    updatePageIndex(db, {
      id: pageId,
      title: "Alice",
      namespace: 0,
      content,
    });

    const bundle = getContextBundle(db, "Alice");
    expect(bundle).not.toBeNull();
    if (!bundle) return;

    expect(bundle.sections.length).toBe(2);
    expect(bundle.templateCalls.length).toBeGreaterThan(0);
    expect(bundle.infobox.length).toBe(2);
    expect(bundle.categories).toContain("People");
    expect(bundle.shortdesc).toBe("Test subject");
  });
});
