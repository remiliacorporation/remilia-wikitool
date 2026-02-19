import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { generateBunSnapshot, type RuntimeSnapshot } from './snapshot-bun.ts';

interface RequestTelemetry {
  request_count: number;
  bytes_sent: number;
  bytes_received: number;
  retry_count: number;
  note: string;
}

interface RuntimeMetrics {
  duration_ms: number;
  request_telemetry: RequestTelemetry;
}

interface DifferentialReport {
  generated_at: string;
  fixture_root: string;
  parity_ok: boolean;
  mismatch_count: number;
  mismatches: string[];
  bun_snapshot_hash: string;
  rust_snapshot_hash: string;
  metrics: {
    bun: RuntimeMetrics;
    rust: RuntimeMetrics;
  };
}

interface BaselineFile {
  schema_version: 1;
  report: DifferentialReport;
}

const repoRoot = resolve(import.meta.dir, '..', '..');
const defaultFixture = resolve(repoRoot, 'tests/fixtures/full-refresh');
const defaultBaselinePath = resolve(repoRoot, 'phase0/baselines/offline-fixture-baseline.json');

const projectRoot = resolve(getArg('--project-root', defaultFixture));
const contentDir = getArg('--content-dir', 'wiki_content');
const templatesDir = getArg('--templates-dir', 'custom/templates');
const baselinePath = resolve(getArg('--baseline-path', defaultBaselinePath));
const writeBaseline = hasFlag('--write-baseline');

const bunStart = performance.now();
const bunSnapshot = generateBunSnapshot({ projectRoot, contentDir, templatesDir });
const bunDuration = roundDuration(performance.now() - bunStart);

const rustStart = performance.now();
const rustSnapshot = generateRustSnapshot(projectRoot, contentDir, templatesDir);
const rustDuration = roundDuration(performance.now() - rustStart);

const mismatches = diffSnapshots(bunSnapshot, rustSnapshot);
const report: DifferentialReport = {
  generated_at: new Date().toISOString(),
  fixture_root: normalize(projectRoot),
  parity_ok: mismatches.length === 0,
  mismatch_count: mismatches.length,
  mismatches,
  bun_snapshot_hash: snapshotHash(bunSnapshot),
  rust_snapshot_hash: snapshotHash(rustSnapshot),
  metrics: {
    bun: {
      duration_ms: bunDuration,
      request_telemetry: offlineTelemetry('offline fixture scan via Bun APIs'),
    },
    rust: {
      duration_ms: rustDuration,
      request_telemetry: offlineTelemetry('offline fixture scan via Rust workspace'),
    },
  },
};

if (writeBaseline) {
  writeBaselineFile(baselinePath, { schema_version: 1, report });
  console.log(`[phase0] baseline updated at ${normalize(baselinePath)}`);
  printSummary(report);
  process.exit(report.parity_ok ? 0 : 1);
}

const baseline = readBaselineFile(baselinePath);
if (baseline.report.bun_snapshot_hash !== report.bun_snapshot_hash) {
  console.error('[phase0] Bun snapshot hash diverged from baseline.');
  console.error(`  baseline: ${baseline.report.bun_snapshot_hash}`);
  console.error(`  current : ${report.bun_snapshot_hash}`);
  process.exit(1);
}

if (baseline.report.rust_snapshot_hash !== report.rust_snapshot_hash) {
  console.error('[phase0] Rust snapshot hash diverged from baseline.');
  console.error(`  baseline: ${baseline.report.rust_snapshot_hash}`);
  console.error(`  current : ${report.rust_snapshot_hash}`);
  process.exit(1);
}

if (!report.parity_ok) {
  console.error('[phase0] Bun/Rust snapshot parity failed.');
  for (const mismatch of report.mismatches) {
    console.error(`  - ${mismatch}`);
  }
  process.exit(1);
}

printSummary(report);

function generateRustSnapshot(projectRoot: string, contentDir: string, templatesDir: string): RuntimeSnapshot {
  const args = [
    'run',
    '--quiet',
    '--package',
    'wikitool',
    '--',
    'phase0',
    'snapshot',
    '--project-root',
    projectRoot,
    '--content-dir',
    contentDir,
    '--templates-dir',
    templatesDir,
  ];

  const result = spawnSync('cargo', args, {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
    encoding: 'utf-8',
  });

  if (result.status !== 0) {
    throw new Error(`Rust snapshot command failed:\n${result.stderr}`);
  }

  return JSON.parse(result.stdout) as RuntimeSnapshot;
}

function snapshotHash(snapshot: RuntimeSnapshot): string {
  const digest = createHash('sha256');
  digest.update(stableStringify(snapshot));
  return digest.digest('hex');
}

function diffSnapshots(left: RuntimeSnapshot, right: RuntimeSnapshot): string[] {
  const mismatches: string[] = [];
  if (left.content_file_count !== right.content_file_count) {
    mismatches.push(`content_file_count mismatch: bun=${left.content_file_count} rust=${right.content_file_count}`);
  }
  if (left.template_file_count !== right.template_file_count) {
    mismatches.push(`template_file_count mismatch: bun=${left.template_file_count} rust=${right.template_file_count}`);
  }

  const rightByPath = new Map(right.files.map((file) => [file.relative_path, file]));
  for (const bunFile of left.files) {
    const rustFile = rightByPath.get(bunFile.relative_path);
    if (!rustFile) {
      mismatches.push(`missing file in rust snapshot: ${bunFile.relative_path}`);
      continue;
    }

    if (bunFile.title !== rustFile.title) {
      mismatches.push(`title mismatch for ${bunFile.relative_path}: bun="${bunFile.title}" rust="${rustFile.title}"`);
    }
    if (bunFile.namespace !== rustFile.namespace) {
      mismatches.push(
        `namespace mismatch for ${bunFile.relative_path}: bun="${bunFile.namespace}" rust="${rustFile.namespace}"`
      );
    }
    if (bunFile.is_redirect !== rustFile.is_redirect) {
      mismatches.push(
        `redirect flag mismatch for ${bunFile.relative_path}: bun=${bunFile.is_redirect} rust=${rustFile.is_redirect}`
      );
    }
    if (bunFile.redirect_target !== rustFile.redirect_target) {
      mismatches.push(
        `redirect target mismatch for ${bunFile.relative_path}: bun="${bunFile.redirect_target}" rust="${rustFile.redirect_target}"`
      );
    }
    if (bunFile.content_hash !== rustFile.content_hash) {
      mismatches.push(
        `content hash mismatch for ${bunFile.relative_path}: bun=${bunFile.content_hash} rust=${rustFile.content_hash}`
      );
    }
  }

  const leftPaths = new Set(left.files.map((file) => file.relative_path));
  for (const rustFile of right.files) {
    if (!leftPaths.has(rustFile.relative_path)) {
      mismatches.push(`unexpected file in rust snapshot: ${rustFile.relative_path}`);
    }
  }

  return mismatches;
}

function writeBaselineFile(path: string, baseline: BaselineFile): void {
  const parent = dirname(path);
  if (!existsSync(parent)) {
    mkdirSync(parent, { recursive: true });
  }
  writeFileSync(path, JSON.stringify(baseline, null, 2) + '\n', 'utf-8');
}

function readBaselineFile(path: string): BaselineFile {
  if (!existsSync(path)) {
    throw new Error(`baseline file not found at ${normalize(path)}; run with --write-baseline first`);
  }
  return JSON.parse(readFileSync(path, 'utf-8')) as BaselineFile;
}

function stableStringify(value: unknown): string {
  return JSON.stringify(sortDeep(value));
}

function sortDeep(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map((entry) => sortDeep(entry));
  }
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    const entries = Object.entries(value as Record<string, unknown>).sort(([left], [right]) =>
      left.localeCompare(right)
    );
    for (const [key, entry] of entries) {
      out[key] = sortDeep(entry);
    }
    return out;
  }
  return value;
}

function offlineTelemetry(note: string): RequestTelemetry {
  return {
    request_count: 0,
    bytes_sent: 0,
    bytes_received: 0,
    retry_count: 0,
    note,
  };
}

function printSummary(report: DifferentialReport): void {
  console.log('[phase0] differential harness summary');
  console.log(`  fixture root: ${report.fixture_root}`);
  console.log(`  parity: ${report.parity_ok ? 'ok' : 'failed'} (${report.mismatch_count} mismatches)`);
  console.log(`  bun hash : ${report.bun_snapshot_hash}`);
  console.log(`  rust hash: ${report.rust_snapshot_hash}`);
  console.log(`  bun duration : ${report.metrics.bun.duration_ms} ms`);
  console.log(`  rust duration: ${report.metrics.rust.duration_ms} ms`);
}

function getArg(flag: string, fallback: string): string {
  const args = process.argv.slice(2);
  const index = args.indexOf(flag);
  if (index === -1 || index + 1 >= args.length) {
    return fallback;
  }
  return args[index + 1];
}

function hasFlag(flag: string): boolean {
  return process.argv.slice(2).includes(flag);
}

function roundDuration(value: number): number {
  return Math.round(value * 100) / 100;
}

function normalize(path: string): string {
  return path.replace(/\\/g, '/');
}
