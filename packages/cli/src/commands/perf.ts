/**
 * perf command - Performance diagnostics helpers
 */

import * as fs from 'node:fs';
import * as path from 'node:path';
import { spawn } from 'node:child_process';
import ora from 'ora';
import { detectProjectContext, withContext } from '../utils/context.js';
import { buildMeta, withMeta } from '../utils/meta.js';
import { formatBytes, printError, printSection, printSuccess, printWarning, printInfo } from '../utils/format.js';
import { isHttpUrl, resolveTargetUrl } from '../utils/url.js';

const DEFAULT_EXPORTS_DIR = 'wikitool_exports';
const USE_DEFAULT_EXPORTS_DIR = !process.env.WIKITOOL_NO_DEFAULT_EXPORTS;

export interface LighthouseOptions {
  output?: string;
  out?: string;
  categories?: string;
  chromeFlags?: string;
  json?: boolean;
  meta?: boolean;
  url?: string;
  showVersion?: boolean;
}

export async function perfLighthouseCommand(target: string | undefined, options: LighthouseOptions = {}): Promise<void> {
  const spinner = ora('Preparing Lighthouse...').start();

  try {
    await withContext(async (ctx) => {
      const project = ctx.projectContext ?? detectProjectContext();
      const lighthousePath = resolveLighthouseBinary(project.wikitoolRoot);
      if (!lighthousePath) {
        spinner.fail('Lighthouse not found');
        printError('Lighthouse not found on PATH or node_modules/.bin.');
        printError('Install with: npm install -g lighthouse');
        process.exit(1);
      }

      if (options.showVersion) {
        const info = await getLighthouseVersionInfo(lighthousePath);
        spinner.stop();

        if (info.code !== 0) {
          printError('Failed to read Lighthouse version');
          if (info.stderr) {
            printError(info.stderr.trim());
          }
          process.exit(info.code);
        }

        if (options.json) {
          const payload = options.meta === false ? info : withMeta(info, buildMeta(ctx));
          console.log(JSON.stringify(payload, null, 2));
          return;
        }

        printSection('Lighthouse');
        console.log(`  Path: ${info.path}`);
        console.log(`  Version: ${info.version ?? 'unknown'}`);
        printSuccess('Lighthouse version resolved');
        return;
      }

      if (!target) {
        spinner.fail('Missing target');
        printError('Target is required unless --show-version is used');
        process.exit(1);
      }

      const url = resolveTargetUrl(target, ctx.db, options.url);
      const outputFormat = normalizeOutputFormat(options.output);
      const outputPath = resolveOutputPath(project.projectRoot, target, url, outputFormat, options.out);

      ensureOutputDir(outputPath);

      const categories = parseList(options.categories);
      let windowsUserDataDir: string | null = null;
      if (process.platform === 'win32') {
        const candidate = resolveWindowsUserDataDir(project.projectRoot);
        try {
          ensureDir(candidate);
          windowsUserDataDir = candidate;
        } catch {
          windowsUserDataDir = null;
        }
      }
      const args = buildLighthouseArgs(
        url,
        outputFormat,
        outputPath,
        categories,
        options.chromeFlags,
        windowsUserDataDir
      );

      spinner.text = 'Running Lighthouse...';
      const runResult = await runProcess(lighthousePath, args, options.json === true);
      spinner.stop();

      if (runResult.code !== 0) {
        if (isIgnorableCleanupFailure(runResult.stderr, outputPath)) {
          printWarning('Lighthouse completed with a known Windows cleanup error');
          printInfo('Report was generated; ignoring chrome-launcher temp cleanup failure');
        } else {
          printError(`Lighthouse exited with code ${runResult.code}`);
          process.exit(runResult.code);
        }
      }

      const reportSize = getFileSize(outputPath);
      const result = {
        url,
        format: outputFormat,
        reportPath: outputPath,
        reportBytes: reportSize,
        categories,
      };

      if (options.json) {
        const output = options.meta === false ? result : withMeta(result, buildMeta(ctx));
        console.log(JSON.stringify(output, null, 2));
        return;
      }

      printSection('Lighthouse');
      console.log(`  URL: ${url}`);
      console.log(`  Format: ${outputFormat}`);
      console.log(`  Report: ${outputPath}`);
      if (reportSize !== null) {
        console.log(`  Size: ${formatBytes(reportSize)}`);
      }
      printSuccess('Lighthouse audit complete');
    });
  } catch (error) {
    spinner.fail('Lighthouse failed');
    const message = error instanceof Error ? error.message : String(error);
    printError(message);
    process.exit(1);
  }
}

function normalizeOutputFormat(format?: string): 'html' | 'json' {
  const raw = (format || 'html').toLowerCase();
  if (raw === 'html' || raw === 'json') return raw;
  throw new Error(`Unsupported output format: ${format || ''}`);
}

function resolveOutputPath(
  projectRoot: string,
  target: string,
  url: string,
  format: 'html' | 'json',
  override?: string
): string {
  if (override) {
    return path.resolve(process.cwd(), override);
  }

  const label = deriveLabel(target, url);
  const slug = sanitizeFilename(label) || 'report';
  const stamp = formatTimestamp(new Date());
  const ext = format === 'json' ? '.json' : '.html';
  const filename = `lighthouse-${slug}-${stamp}${ext}`;

  if (USE_DEFAULT_EXPORTS_DIR) {
    return path.join(projectRoot, DEFAULT_EXPORTS_DIR, filename);
  }

  return path.join(process.cwd(), filename);
}

function deriveLabel(target: string, url: string): string {
  if (isHttpUrl(target)) {
    try {
      const parsed = new URL(url);
      const segment = lastPathSegment(parsed.pathname);
      if (segment) {
        try {
          return decodeURIComponent(segment);
        } catch {
          return segment;
        }
      }
      return parsed.hostname || target;
    } catch {
      return target;
    }
  }
  return target;
}

function lastPathSegment(pathname: string): string {
  let end = pathname.length - 1;
  while (end >= 0 && pathname[end] === '/') end--;
  if (end < 0) return '';

  let start = end;
  while (start >= 0 && pathname[start] !== '/') start--;
  return pathname.slice(start + 1, end + 1);
}

function sanitizeFilename(value: string): string {
  let out = '';
  let prevDash = false;

  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    if (isWhitespace(ch)) {
      if (!prevDash && out.length > 0) {
        out += '-';
        prevDash = true;
      }
      continue;
    }
    if (isInvalidFilenameChar(ch)) {
      if (!prevDash && out.length > 0) {
        out += '-';
        prevDash = true;
      }
      continue;
    }
    out += ch;
    prevDash = false;
  }

  if (out.endsWith('-')) {
    out = trimTrailingDash(out);
  }

  return out;
}

function trimTrailingDash(value: string): string {
  let end = value.length;
  while (end > 0 && value[end - 1] === '-') end--;
  return value.slice(0, end);
}

function isInvalidFilenameChar(ch: string): boolean {
  return ch === '<' || ch === '>' || ch === ':' || ch === '"' || ch === '|' ||
    ch === '?' || ch === '*' || ch === '/' || ch === '\\';
}

function isWhitespace(ch: string): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r';
}

function formatTimestamp(date: Date): string {
  const year = date.getFullYear();
  const month = pad2(date.getMonth() + 1);
  const day = pad2(date.getDate());
  const hour = pad2(date.getHours());
  const min = pad2(date.getMinutes());
  const sec = pad2(date.getSeconds());
  return `${year}${month}${day}-${hour}${min}${sec}`;
}

function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

function parseList(value?: string): string[] {
  if (!value) return [];
  const list: string[] = [];
  let current = '';

  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    if (ch === ',') {
      const trimmed = trimWhitespace(current);
      if (trimmed) list.push(trimmed);
      current = '';
      continue;
    }
    current += ch;
  }

  const last = trimWhitespace(current);
  if (last) list.push(last);
  return list;
}

function trimWhitespace(value: string): string {
  let start = 0;
  let end = value.length;
  while (start < end && isWhitespace(value[start])) start++;
  while (end > start && isWhitespace(value[end - 1])) end--;
  return value.slice(start, end);
}

function buildLighthouseArgs(
  url: string,
  format: 'html' | 'json',
  outputPath: string,
  categories: string[],
  chromeFlags?: string,
  windowsUserDataDir?: string | null
): string[] {
  const args = [url, '--output', format, '--output-path', outputPath, '--quiet'];

  if (categories.length > 0) {
    args.push(`--only-categories=${categories.join(',')}`);
  }

  // Build chrome flags - on Windows, set a custom user-data-dir to avoid
  // chrome-launcher temp directory permission issues (EPERM on cleanup)
  const flags: string[] = [];
  if (chromeFlags && chromeFlags.trim().length > 0) {
    flags.push(chromeFlags.trim());
  }
  const hasUserDataDir = chromeFlags
    ? chromeFlags.toLowerCase().includes('--user-data-dir')
    : false;
  if (windowsUserDataDir && !hasUserDataDir) {
    flags.push(`--user-data-dir=${windowsUserDataDir}`);
    flags.push('--no-first-run');
    flags.push('--no-default-browser-check');
  }
  if (flags.length > 0) {
    args.push(`--chrome-flags=${flags.join(' ')}`);
  }

  return args;
}

async function getLighthouseVersionInfo(lighthousePath: string): Promise<{
  path: string;
  version: string | null;
  code: number;
  stderr: string;
}> {
  const result = await runProcess(lighthousePath, ['--version'], true);
  if (result.code !== 0) {
    return { path: lighthousePath, version: null, code: result.code, stderr: result.stderr };
  }
  const version = extractFirstLine(result.stdout);
  return { path: lighthousePath, version, code: result.code, stderr: result.stderr };
}

function extractFirstLine(value: string): string | null {
  const lines = splitLines(value);
  for (const line of lines) {
    const trimmed = trimWhitespace(line);
    if (trimmed.length > 0) {
      return trimmed;
    }
  }
  return null;
}

function resolveWindowsUserDataDir(projectRoot: string): string {
  const override = trimWhitespace(process.env.WIKITOOL_LIGHTHOUSE_USER_DATA_DIR || '');
  if (override) {
    return path.resolve(override);
  }

  const candidates = [
    process.env.PUBLIC,
    process.env.ProgramData,
    process.env.LOCALAPPDATA,
    process.env.TEMP,
    process.env.TMP,
    projectRoot,
  ];

  let base: string | null = null;
  for (const candidate of candidates) {
    if (!candidate) continue;
    if (!hasWhitespace(candidate)) {
      base = candidate;
      break;
    }
  }

  if (!base) {
    for (const candidate of candidates) {
      if (candidate && trimWhitespace(candidate).length > 0) {
        base = candidate;
        break;
      }
    }
  }

  return path.join(base || projectRoot, 'wikitool-lighthouse');
}

function hasWhitespace(value: string): boolean {
  for (let i = 0; i < value.length; i++) {
    if (isWhitespace(value[i])) return true;
  }
  return false;
}

function ensureDir(dir: string): void {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

function ensureOutputDir(outputPath: string): void {
  const dir = path.dirname(outputPath);
  ensureDir(dir);
}

function filterChromeLauncherOutput(stderr: string): string {
  if (process.platform !== 'win32') return stderr;
  if (!stderr) return stderr;

  const lines = splitLines(stderr);
  const kept: string[] = [];
  let suppressing = false;

  for (const line of lines) {
    if (suppressing) {
      const trimmed = trimWhitespace(line);
      if (trimmed.length === 0 || isStackLine(line)) {
        continue;
      }
      suppressing = false;
    }

    if (isChromeLauncherCleanupLine(line)) {
      suppressing = true;
      continue;
    }

    kept.push(line);
  }

  return kept.join('\n');
}

function isIgnorableCleanupFailure(stderr: string, outputPath: string): boolean {
  if (process.platform !== 'win32') return false;
  if (!stderr) return false;
  if (!fs.existsSync(outputPath)) return false;
  const size = getFileSize(outputPath);
  if (size === null || size === 0) return false;

  const lower = stderr.toLowerCase();
  if (!lower.includes('eperm')) return false;
  if (!lower.includes('chrome-launcher')) return false;
  if (!lower.includes('lighthouse')) return false;
  return true;
}

function splitLines(value: string): string[] {
  const lines: string[] = [];
  let current = '';

  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    if (ch === '\r') {
      if (i + 1 < value.length && value[i + 1] === '\n') {
        i++;
      }
      lines.push(current);
      current = '';
      continue;
    }
    if (ch === '\n') {
      lines.push(current);
      current = '';
      continue;
    }
    current += ch;
  }

  lines.push(current);
  return lines;
}

function isStackLine(line: string): boolean {
  let i = 0;
  while (i < line.length && isWhitespace(line[i])) i++;
  return line.startsWith('at ', i);
}

function isChromeLauncherCleanupLine(line: string): boolean {
  const lower = line.toLowerCase();
  if (lower.includes('chrome-launcher')) {
    if (lower.includes('eperm')) return true;
    if (lower.includes('cleanup')) return true;
    if (lower.includes('failed') && lower.includes('remove')) return true;
    if (lower.includes('failed') && lower.includes('delete')) return true;
  }

  if (lower.includes('runtime error encountered') && lower.includes('eperm') && lower.includes('lighthouse')) {
    return true;
  }
  if (lower.includes('permission denied') && lower.includes('lighthouse')) {
    return true;
  }

  return false;
}

function runProcess(
  command: string,
  args: string[],
  suppressOutput: boolean
): Promise<{ code: number; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';

    if (child.stdout) {
      child.stdout.on('data', (chunk) => {
        stdout += chunk.toString();
      });
    }
    if (child.stderr) {
      child.stderr.on('data', (chunk) => {
        stderr += chunk.toString();
      });
    }

    child.on('error', () => resolve({ code: 1, stdout, stderr }));
    child.on('close', (code) => {
      if (!suppressOutput) {
        if (stdout.length > 0) {
          process.stdout.write(stdout);
        }
        const filtered = filterChromeLauncherOutput(stderr);
        if (filtered.length > 0) {
          process.stderr.write(filtered);
        }
      }
      resolve({ code: code ?? 1, stdout, stderr });
    });
  });
}

function resolveLighthouseBinary(wikitoolRoot: string): string | null {
  const envPath = process.env.LIGHTHOUSE_PATH;
  if (envPath && fs.existsSync(envPath)) {
    return envPath;
  }

  const isWin = process.platform === 'win32';
  const names = isWin
    ? ['lighthouse.cmd', 'lighthouse.exe', 'lighthouse']
    : ['lighthouse'];

  const localBin = path.resolve(wikitoolRoot, 'node_modules', '.bin');
  for (const name of names) {
    const full = path.join(localBin, name);
    if (fs.existsSync(full)) return full;
  }

  const pathEnv = process.env.PATH || '';
  const separator = isWin ? ';' : ':';
  const paths = splitPathList(pathEnv, separator);

  for (const entry of paths) {
    const trimmed = stripQuotes(trimWhitespace(entry));
    if (!trimmed) continue;
    for (const name of names) {
      const candidate = path.join(trimmed, name);
      if (fs.existsSync(candidate)) return candidate;
    }
  }

  return null;
}

function splitPathList(value: string, separator: string): string[] {
  const parts: string[] = [];
  let current = '';

  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    if (ch === separator) {
      parts.push(current);
      current = '';
    } else {
      current += ch;
    }
  }

  parts.push(current);
  return parts;
}

function stripQuotes(value: string): string {
  if (value.length >= 2 && value[0] === '"' && value[value.length - 1] === '"') {
    return value.slice(1, -1);
  }
  return value;
}

function getFileSize(filePath: string): number | null {
  try {
    const stats = fs.statSync(filePath);
    return stats.size;
  } catch {
    return null;
  }
}
