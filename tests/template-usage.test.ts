import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  createDatabase,
  updatePageIndex,
  getTemplateUsageStats,
  getTemplateSchema,
} from "../packages/core/src/index.ts";

describe("template usage analytics", () => {
  let db: Awaited<ReturnType<typeof createDatabase>>;

  beforeAll(async () => {
    db = await createDatabase(":memory:");
  });

  afterAll(() => {
    db?.close();
  });

  test("aggregates usage and schema", () => {
    const templateContent = [
      "<templatedata>",
      "{",
      "  \"params\": {",
      "    \"name\": { \"required\": true, \"description\": \"Person name\" },",
      "    \"age\": { \"type\": \"number\" }",
      "  }",
      "}",
      "</templatedata>",
    ].join("\n");

    const templateId = db.upsertPage({
      title: "Template:Infobox person",
      namespace: 10,
      page_type: "template",
      filename: "Infobox person.wiki",
      filepath: "wiki_content/Template/Infobox person.wiki",
      content: templateContent,
      content_hash: "hash-template",
      sync_status: "synced",
    });

    updatePageIndex(db, {
      id: templateId,
      title: "Template:Infobox person",
      namespace: 10,
      content: templateContent,
    });

    const pageOne = "{{Infobox person|name=Alice|age=30}}";
    const pageOneId = db.upsertPage({
      title: "Alice",
      namespace: 0,
      page_type: "article",
      filename: "Alice.wiki",
      filepath: "wiki_content/Main/Alice.wiki",
      content: pageOne,
      content_hash: "hash-alice",
      sync_status: "synced",
    });
    updatePageIndex(db, { id: pageOneId, title: "Alice", namespace: 0, content: pageOne });

    const pageTwo = "{{Infobox person|name=Bob}}\n{{Infobox person|Bob|age=25}}";
    const pageTwoId = db.upsertPage({
      title: "Bob",
      namespace: 0,
      page_type: "article",
      filename: "Bob.wiki",
      filepath: "wiki_content/Main/Bob.wiki",
      content: pageTwo,
      content_hash: "hash-bob",
      sync_status: "synced",
    });
    updatePageIndex(db, { id: pageTwoId, title: "Bob", namespace: 0, content: pageTwo });

    const usage = getTemplateUsageStats(db, "Infobox person");
    expect(usage.totalCalls).toBe(3);
    expect(usage.totalPages).toBe(2);

    const nameParam = usage.namedParams.find(p => p.name === "name");
    expect(nameParam?.usageCount).toBe(2);
    expect(nameParam?.pageCount).toBe(2);

    const ageParam = usage.namedParams.find(p => p.name === "age");
    expect(ageParam?.usageCount).toBe(2);

    const positional = usage.positionalParams.find(p => p.index === 1);
    expect(positional?.usageCount).toBe(1);

    const schema = getTemplateSchema(db, "Infobox person", usage);
    const schemaName = schema.params.find(p => p.name === "name");
    expect(schemaName?.required).toBe(true);
    expect(schema.source).toBe("merged");
  });
});