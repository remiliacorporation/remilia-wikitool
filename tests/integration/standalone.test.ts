import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { initCommand } from "../../packages/cli/src/commands/init.ts";

const INTEGRATION = process.env.WIKITOOL_INTEGRATION === "1";
const ENV_KEYS = ["WIKITOOL_PROJECT_ROOT", "WIKITOOL_ROOT", "WIKITOOL_DB"] as const;

let tempRoot = "";
let envSnapshot: Record<string, string | undefined> = {};

function snapshotEnv(): void {
  envSnapshot = {};
  for (const key of ENV_KEYS) {
    envSnapshot[key] = process.env[key];
  }
}

function restoreEnv(): void {
  for (const key of ENV_KEYS) {
    const value = envSnapshot[key];
    if (value === undefined) {
      delete process.env[key];
    } else {
      process.env[key] = value;
    }
  }
}

function writeJson(filepath: string, data: unknown): void {
  mkdirSync(dirname(filepath), { recursive: true });
  writeFileSync(filepath, JSON.stringify(data), "utf8");
}

function createWikitoolSkeleton(root: string): void {
  writeJson(join(root, "package.json"), { name: "wikitool" });
  writeJson(join(root, "packages", "core", "package.json"), { name: "@wikitool/core" });
  writeJson(join(root, "packages", "cli", "package.json"), { name: "@wikitool/cli" });
}

describe.skipIf(!INTEGRATION)("Integration: standalone init", () => {
  beforeEach(() => {
    snapshotEnv();
    tempRoot = mkdtempSync(join(tmpdir(), "wikitool-integration-"));
  });

  afterEach(() => {
    restoreEnv();
    if (tempRoot && existsSync(tempRoot)) {
      rmSync(tempRoot, { recursive: true, force: true });
    }
  });

  test("init creates sibling directories and env template", async () => {
    const projectRoot = join(tempRoot, "project");
    const wikitoolRoot = join(projectRoot, "wikitool");
    createWikitoolSkeleton(wikitoolRoot);

    const dbPath = join(wikitoolRoot, "data", "wikitool.test.db");

    process.env.WIKITOOL_PROJECT_ROOT = projectRoot;
    process.env.WIKITOOL_ROOT = "wikitool";
    process.env.WIKITOOL_DB = dbPath;

    await initCommand({});

    expect(existsSync(join(projectRoot, "wiki_content", "Main"))).toBe(true);
    expect(existsSync(join(projectRoot, "wiki_content", "Category"))).toBe(true);
    expect(existsSync(join(projectRoot, "templates"))).toBe(true);
    expect(existsSync(join(wikitoolRoot, ".env.template"))).toBe(true);
  });
});
