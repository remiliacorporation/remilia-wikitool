import { describe, expect, test } from "bun:test";
import {
  parseSections,
  parseTemplateCalls,
  parseTemplateData,
  parseModuleDependencies,
} from "../packages/core/src/parser/context.ts";

describe("context parser", () => {
  test("parses lead and headings deterministically", () => {
    const content = [
      "Lead line 1",
      "",
      "== History ==",
      "Body A",
      "=== Details ===",
      "Body B",
    ].join("\n");

    const sections = parseSections(content);
    expect(sections.length).toBe(3);
    expect(sections[0].isLead).toBe(true);
    expect(sections[1].heading).toBe("History");
    expect(sections[2].heading).toBe("Details");
  });

  test("parses template calls with params", () => {
    const content = "{{Infobox person|name=Alice|birth=1990}}{{Citation|1=Foo|url=bar}}";
    const calls = parseTemplateCalls(content);
    expect(calls.length).toBe(2);
    expect(calls[0].name).toBe("Infobox person");
    expect(calls[0].params.length).toBe(2);
    expect(calls[0].params[0].name).toBe("name");
    expect(calls[1].name).toBe("Citation");
  });

  test("parses templatedata json", () => {
    const content = "<templatedata>{\"params\":{\"foo\":{\"label\":\"Foo\"}},\"description\":\"Test\"}</templatedata>";
    const data = parseTemplateData(content);
    expect(data?.source).toBe("templatedata");
    expect(data?.paramDefs).toContain("foo");
    expect(data?.description).toBe("Test");
  });

  test("parses module dependencies", () => {
    const content = "local m = require('Module:Foo')\\nlocal d = mw.loadData('Bar')";
    const deps = parseModuleDependencies(content);
    expect(deps.length).toBe(2);
    expect(deps[0].dependency).toBe("Module:Foo");
    expect(deps[1].dependency).toBe("Module:Bar");
  });
});
