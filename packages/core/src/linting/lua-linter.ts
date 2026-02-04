/**
 * Lua linting via Selene
 */

import { existsSync, mkdirSync } from 'node:fs';
import { unlink } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { $ } from 'bun';
import type { Database } from '../storage/sqlite.js';

export interface LuaLintResult {
  title: string;
  errors: LuaLintError[];
  warnings: LuaLintWarning[];
}

export interface LuaLintError {
  line: number;
  column: number;
  endLine?: number;
  endColumn?: number;
  code: string;
  message: string;
  severity: 'error' | 'warning';
}

export interface LuaLintWarning {
  line: number;
  column: number;
  endLine?: number;
  endColumn?: number;
  code: string;
  message: string;
  severity: 'error' | 'warning';
}

const SCRATCH_DIR = join(tmpdir(), 'wikitool-lint');

export function isSeleneAvailable(): boolean {
  return getSelenePath() !== null;
}

export async function lintLuaContent(content: string, title: string): Promise<LuaLintResult> {
  const selenePath = getSelenePath();
  if (!selenePath) {
    return { title, errors: [], warnings: [] };
  }

  ensureScratchDir();
  const tempFile = join(SCRATCH_DIR, `${sanitizeTitle(title)}.lua`);
  await Bun.write(tempFile, content);

  const configPath = getConfigPath();
  const configArg = configPath ? ['--config', configPath] : [];

  try {
    const result = await $`${selenePath} ${tempFile} --display-style json ${configArg}`.quiet();
    return parseSeleneOutput(result.stdout.toString(), title);
  } catch (error: unknown) {
    const stdout = (error as { stdout?: { toString?: () => string } })?.stdout?.toString?.() ?? '';
    const stderr = (error as { stderr?: { toString?: () => string } })?.stderr?.toString?.() ?? '';
    return parseSeleneOutput(stdout || stderr, title);
  } finally {
    try {
      await unlink(tempFile);
    } catch {
      // ignore cleanup errors
    }
  }
}

export async function lintAllModules(db: Database): Promise<LuaLintResult[]> {
  const pages = db.getPages({ namespace: 828 });
  const results: LuaLintResult[] = [];
  const concurrency = 4;

  for (let i = 0; i < pages.length; i += concurrency) {
    const batch = pages.slice(i, i + concurrency);
    const batchResults = await Promise.all(
      batch.map(async (page) => {
        const content = page.content ?? '';
        return lintLuaContent(content, page.title);
      })
    );
    for (const result of batchResults) {
      if (result.errors.length > 0 || result.warnings.length > 0) {
        results.push(result);
      }
    }
  }

  return results;
}

function parseSeleneOutput(output: string, title: string): LuaLintResult {
  if (!output || !output.trim()) {
    return { title, errors: [], warnings: [] };
  }

  // Selene outputs one JSON object per line (newline-delimited JSON)
  // followed by a summary line "Results:\n0 errors\n..."
  const diagnostics: unknown[] = [];
  const lines = output.split('\n');

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    // Skip summary lines
    if (trimmed.startsWith('Results:')) break;
    if (trimmed.match(/^\d+ (errors?|warnings?|parse errors?)$/)) continue;

    // Try to parse as JSON
    if (trimmed.startsWith('{')) {
      try {
        diagnostics.push(JSON.parse(trimmed));
      } catch {
        // Not valid JSON, skip
      }
    }
  }

  // If no line-by-line JSON, try parsing entire output as single JSON
  if (diagnostics.length === 0) {
    try {
      const parsed = JSON.parse(output);
      if (Array.isArray(parsed)) {
        diagnostics.push(...parsed);
      } else if (Array.isArray((parsed as { diagnostics?: unknown }).diagnostics)) {
        diagnostics.push(...(parsed as { diagnostics: unknown[] }).diagnostics);
      } else if (Array.isArray((parsed as { results?: unknown }).results)) {
        diagnostics.push(...(parsed as { results: unknown[] }).results);
      }
    } catch {
      // Not valid JSON at all - no diagnostics found (clean file)
    }
  }

  const errors: LuaLintError[] = [];
  const warnings: LuaLintWarning[] = [];

  for (const entry of diagnostics) {
    if (!entry || typeof entry !== 'object') continue;
    const diag = entry as Record<string, unknown>;

    const severity = normalizeSeverity(diag.severity ?? diag.level ?? diag.kind ?? diag.type);
    const code = readString(diag.code ?? diag.rule) || 'selene';
    const message = readString(diag.message ?? diag.msg) || 'Lua lint issue';

    // Selene v0.30+ uses primary_label.span with 0-based line numbers
    const primaryLabel = diag.primary_label as Record<string, unknown> | undefined;
    const span = primaryLabel?.span as Record<string, unknown> | undefined;

    const line = readNumber(
      diag.line,
      diag.startLine,
      diag.start_line,
      span?.start_line,
      (diag.position as Record<string, unknown> | undefined)?.line
    );
    // Selene uses 0-based line numbers, convert to 1-based
    const lineNum = line !== undefined ? line + 1 : 1;

    const column = readNumber(
      diag.column,
      diag.col,
      diag.startColumn,
      diag.start_column,
      span?.start_column,
      (diag.position as Record<string, unknown> | undefined)?.col
    );
    const colNum = column !== undefined ? column + 1 : 1;

    const endLine = readNumber(
      diag.endLine,
      diag.end_line,
      span?.end_line
    );
    const endLineNum = endLine !== undefined ? endLine + 1 : undefined;

    const endColumn = readNumber(
      diag.endColumn,
      diag.end_column,
      span?.end_column
    );
    const endColNum = endColumn !== undefined ? endColumn + 1 : undefined;

    const item: LuaLintError = {
      line: lineNum,
      column: colNum,
      endLine: endLineNum,
      endColumn: endColNum,
      code,
      message,
      severity,
    };

    if (severity === 'error') {
      errors.push(item);
    } else {
      warnings.push(item);
    }
  }

  return { title, errors, warnings };
}

function normalizeSeverity(value: unknown): 'error' | 'warning' {
  const text = typeof value === 'string' ? value.toLowerCase() : '';
  return text.includes('error') ? 'error' : 'warning';
}

function readString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}

function readNumber(...values: unknown[]): number | undefined {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) return value;
  }
  return undefined;
}

function sanitizeTitle(title: string): string {
  let out = '';
  for (let i = 0; i < title.length; i++) {
    const ch = title[i];
    const code = ch.charCodeAt(0);
    const isAlphaNum = (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
    if (isAlphaNum || ch === '-' || ch === '_') {
      out += ch;
    } else {
      out += '_';
    }
  }
  if (!out) return 'module';
  if (out.length > 120) return out.slice(0, 120);
  return out;
}

function ensureScratchDir(): void {
  if (!existsSync(SCRATCH_DIR)) {
    mkdirSync(SCRATCH_DIR, { recursive: true });
  }
}

function getConfigPath(): string | null {
  const envPath = process.env.SELENE_CONFIG_PATH;
  if (envPath && existsSync(envPath)) return envPath;

  let dir = process.cwd();
  while (dir !== dirname(dir)) {
    const candidate = resolve(dir, 'custom', 'wikitool', 'config', 'selene.toml');
    if (existsSync(candidate)) return candidate;
    const local = resolve(dir, 'config', 'selene.toml');
    if (existsSync(local)) return local;
    dir = dirname(dir);
  }

  return null;
}

function getSelenePath(): string | null {
  const envPath = process.env.SELENE_PATH;
  if (envPath && existsSync(envPath)) return envPath;

  const configPath = getConfigPath();
  if (configPath) {
    const root = dirname(dirname(configPath));
    const toolsDir = resolve(root, 'tools');
    const binary = getSeleneBinaryName();
    const localPath = join(toolsDir, binary);
    if (existsSync(localPath)) return localPath;
  }

  return findOnPath('selene');
}

function getSeleneBinaryName(): string {
  const platform = process.platform;

  // Simplified naming - setup scripts now install as selene[.exe]
  if (platform === 'win32') return 'selene.exe';
  return 'selene';
}

function findOnPath(command: string): string | null {
  const pathVar = process.env.PATH;
  if (!pathVar) return null;

  const parts = pathVar.split(process.platform === 'win32' ? ';' : ':');
  const candidates = process.platform === 'win32' ? [command, `${command}.exe`, `${command}.cmd`] : [command];

  for (const part of parts) {
    for (const name of candidates) {
      const full = join(part, name);
      if (existsSync(full)) return full;
    }
  }

  return null;
}
