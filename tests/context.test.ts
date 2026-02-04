import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { detectProjectContext } from "../packages/cli/src/utils/context.ts";

const ENV_KEYS = ["WIKITOOL_PROJECT_ROOT", "WIKITOOL_ROOT", "WIKITOOL_DB"] as const;

let tempDirs: string[] = [];
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

function makeTempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function writeJson(filepath: string, data: unknown): void {
  mkdirSync(dirname(filepath), { recursive: true });
  writeFileSync(filepath, JSON.stringify(data), "utf8");
}

function createWikitoolRoot(root: string): void {
  writeJson(join(root, "package.json"), { name: "wikitool" });
  writeJson(join(root, "packages", "core", "package.json"), { name: "@wikitool/core" });
  writeJson(join(root, "packages", "cli", "package.json"), { name: "@wikitool/cli" });
}

function createEmbeddedFixture(projectRoot: string): string {
  const wikitoolRoot = join(projectRoot, "custom", "wikitool");
  createWikitoolRoot(wikitoolRoot);
  return wikitoolRoot;
}

function createStandaloneFixture(projectRoot: string): string {
  const wikitoolRoot = join(projectRoot, "wikitool");
  createWikitoolRoot(wikitoolRoot);
  return wikitoolRoot;
}

beforeEach(() => {
  snapshotEnv();
});

afterEach(() => {
  restoreEnv();
  for (const dir of tempDirs) {
    rmSync(dir, { recursive: true, force: true });
  }
  tempDirs = [];
});

describe("detectProjectContext", () => {
  test("returns embedded mode when custom/wikitool/package.json exists", () => {
    const projectRoot = makeTempDir("wikitool-embedded-");
    createEmbeddedFixture(projectRoot);

    const ctx = detectProjectContext(projectRoot);
    expect(ctx.mode).toBe("embedded");
    expect(ctx.projectRoot).toBe(projectRoot);
    expect(ctx.wikitoolRoot.endsWith(join("custom", "wikitool"))).toBe(true);
    expect(ctx.templatesDirRel).toBe("custom/templates");
    expect(ctx.contentDirRel).toBe("wiki_content");
  });

  test("returns standalone mode when wikitool/package.json exists", () => {
    const projectRoot = makeTempDir("wikitool-standalone-");
    createStandaloneFixture(projectRoot);

    const ctx = detectProjectContext(projectRoot);
    expect(ctx.mode).toBe("standalone");
    expect(ctx.projectRoot).toBe(projectRoot);
    expect(ctx.wikitoolRoot.endsWith(join("wikitool"))).toBe(true);
    expect(ctx.templatesDirRel).toBe("templates");
    expect(ctx.contentDirRel).toBe("wiki_content");
  });

  test("respects WIKITOOL_PROJECT_ROOT env override", () => {
    const projectRoot = makeTempDir("wikitool-env-");
    createStandaloneFixture(projectRoot);

    process.env.WIKITOOL_PROJECT_ROOT = projectRoot;
    const ctx = detectProjectContext(makeTempDir("wikitool-start-"));
    expect(ctx.projectRoot).toBe(projectRoot);
  });

  test("finds wikitool root from project root with child wikitool/", () => {
    const projectRoot = makeTempDir("wikitool-child-");
    createStandaloneFixture(projectRoot);

    const ctx = detectProjectContext(projectRoot);
    expect(ctx.wikitoolRoot).toBe(join(projectRoot, "wikitool"));
    expect(ctx.projectRoot).toBe(projectRoot);
  });

  test("throws a helpful error when no wikitool root is found", () => {
    const emptyRoot = makeTempDir("wikitool-empty-");
    expect(() => detectProjectContext(emptyRoot)).toThrow(
      "Could not locate wikitool root"
    );
  });
});
