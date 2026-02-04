/**
 * JSON meta helper for CLI outputs
 */

import { VERSION } from '@wikitool/core';
import { detectProjectContext, type CliContext } from './context.js';

export interface MetaBlock {
  tool: {
    name: string;
    version: string;
    root: string;
    execPath: string;
    language: string;
    compiler: string;
    runtime: string;
  };
  context: {
    cwd: string;
    configPath: string;
    dbPath: string;
    command: string;
    timestamp: string;
  };
}

export function buildMeta(ctx: CliContext, command?: string): MetaBlock {
  const project = ctx.projectContext ?? detectProjectContext();
  return {
    tool: {
      name: 'wikitool',
      version: VERSION,
      root: project.projectRoot,
      execPath: process.execPath,
      language: 'TypeScript',
      compiler: 'bun',
      runtime: `Bun ${Bun.version}`,
    },
    context: {
      cwd: process.cwd(),
      configPath: project.configPath,
      dbPath: project.dbPath,
      command: command ?? process.argv.join(' '),
      timestamp: new Date().toISOString(),
    },
  };
}

export function withMeta<T>(data: T, meta: MetaBlock): T & { meta: MetaBlock } | { meta: MetaBlock; data: T } {
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    return { meta, ...(data as object) } as T & { meta: MetaBlock };
  }

  return { meta, data };
}
