import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { detectProjectContext } from '../packages/cli/src/utils/context.ts';

type HelpResult = {
  usage: string;
  options: string[];
  subcommands: string[];
};

const ctx = detectProjectContext(process.cwd());
const outputDir = join(ctx.projectRoot, 'docs', 'wikitool');
const outputFile = join(outputDir, 'reference.md');
const cliDist = resolve(ctx.wikitoolRoot, 'packages', 'cli', 'dist', 'index.js');

function runHelp(args: string[]): string {
  if (!existsSync(cliDist)) {
    throw new Error('CLI not built. Run `bun run build` before generating reference.');
  }
  const result = spawnSync('bun', ['run', 'wikitool', 'help', ...args], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    cwd: ctx.wikitoolRoot,
  });

  if (result.status !== 0) {
    const err = result.stderr?.trim() || 'Unknown error';
    throw new Error(`Failed to run wikitool help ${args.join(' ')}: ${err}`);
  }

  return (result.stdout || '').trimEnd();
}

function parseUsage(helpText: string): string {
  const match = helpText.match(/^Usage:\s*(.+)$/m);
  if (!match) {
    return 'wikitool';
  }
  return match[1].trim();
}

function parseOptions(helpText: string, includeHelp: boolean): string[] {
  const lines = helpText.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === 'Options:');
  if (start === -1) {
    return [];
  }

  const options: string[] = [];
  let sawOption = false;
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (!line.trim()) {
      if (sawOption) {
        break;
      }
      continue;
    }
    if (line.trim() === 'Commands:') {
      break;
    }
    const isOptionLine = /^\s*-/.test(line);
    if (!isOptionLine) {
      if (options.length > 0 && line.trim()) {
        options[options.length - 1] = `${options[options.length - 1]} ${line.trim()}`;
      }
      continue;
    }
    const match = line.match(/^\s*(.+?)\s{2,}(.*)$/);
    if (match) {
      const optionText = `- \`${match[1].trim()}\` ${match[2].trim()}`.trim();
      if (!includeHelp && optionText.includes('--help')) {
        continue;
      }
      sawOption = true;
      options.push(optionText);
    } else {
      const optionText = `- \`${line.trim()}\``;
      if (!includeHelp && optionText.includes('--help')) {
        continue;
      }
      sawOption = true;
      options.push(optionText);
    }
  }

  return options;
}

function parseCommands(helpText: string): string[] {
  const lines = helpText.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === 'Commands:');
  if (start === -1) {
    return [];
  }

  const commands: string[] = [];
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (!line.trim()) {
      if (commands.length > 0) {
        break;
      }
      continue;
    }
    if (line.trim() === 'Options:') {
      break;
    }
    const match = line.match(/^\s*([a-zA-Z0-9:-]+)\s{2,}/);
    if (match) {
      commands.push(match[1]);
    }
  }

  return commands.filter((command) => command !== 'help');
}

function getHelp(args: string[], includeHelp: boolean): HelpResult {
  const helpText = runHelp(args);
  return {
    usage: parseUsage(helpText),
    options: parseOptions(helpText, includeHelp),
    subcommands: parseCommands(helpText),
  };
}

function renderSection(title: string, usage: string, options: string[]): string[] {
  const lines: string[] = [];
  lines.push(`## ${title}`);
  lines.push('');
  lines.push('```');
  lines.push(usage);
  lines.push('```');

  if (options.length > 0) {
    lines.push('');
    lines.push('Options:');
    lines.push(...options);
  }

  lines.push('');
  return lines;
}

function main(): void {
  const top = getHelp([], true);
  const topCommands = top.subcommands;

  const lines: string[] = [];
  lines.push('# Wikitool Command Reference');
  lines.push('');
  lines.push('This file is generated from CLI help output. Do not edit manually.');
  lines.push('It is the canonical command/flag reference for wikitool and is intended for humans and agents.');
  lines.push('');
  lines.push('Regenerate (from `<wikitool-dir>`):');
  lines.push('');
  lines.push('```bash');
  lines.push('bun run docs:reference');
  lines.push('```');
  lines.push('');
  lines.push('You can also view CLI help directly:');
  lines.push('');
  lines.push('```bash');
  lines.push('bun run wikitool help');
  lines.push('bun run wikitool help <command>');
  lines.push('```');
  lines.push('');

  lines.push(...renderSection('Global', top.usage, top.options));

  for (const command of topCommands) {
    const help = getHelp([command], false);
    lines.push(...renderSection(command, help.usage, help.options));

    for (const subcommand of help.subcommands) {
      const subHelp = getHelp([command, subcommand], false);
      lines.push(...renderSection(`${command} ${subcommand}`, subHelp.usage, subHelp.options));
    }
  }

  mkdirSync(outputDir, { recursive: true });
  writeFileSync(outputFile, lines.join('\n'), 'utf8');
}

main();
