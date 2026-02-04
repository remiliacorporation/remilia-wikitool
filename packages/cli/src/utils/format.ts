/**
 * CLI output formatting utilities
 */

import chalk from 'chalk';
import Table from 'cli-table3';

/** Change type formatting */
export const CHANGE_COLORS: Record<string, (text: string) => string> = {
  new_local: chalk.green,
  new_remote: chalk.cyan,
  modified_local: chalk.yellow,
  modified_remote: chalk.blue,
  conflict: chalk.red,
  deleted_local: chalk.magenta,
  deleted_remote: chalk.gray,
  synced: chalk.dim,
};

export const CHANGE_SYMBOLS: Record<string, string> = {
  new_local: 'N',
  new_remote: 'R',
  modified_local: 'M',
  modified_remote: 'U',
  conflict: 'C',
  deleted_local: 'D',
  deleted_remote: 'X',
  synced: ' ',
};

/**
 * Format a change for display
 */
export function formatChange(type: string, title: string, filepath?: string): string {
  const color = CHANGE_COLORS[type] || chalk.white;
  const symbol = CHANGE_SYMBOLS[type] || '?';
  const displayPath = filepath || title;
  return `  ${color(`[${symbol}]`)} ${displayPath}`;
}

/**
 * Format file size in human-readable form (compact, no spaces)
 */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

/**
 * Format bytes in human-readable form (with spaces, includes GB)
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/**
 * Format a timestamp
 */
export function formatTime(timestamp: string | null | undefined): string {
  if (!timestamp) return chalk.dim('never');
  try {
    const date = new Date(timestamp);
    return date.toLocaleString();
  } catch {
    return timestamp;
  }
}

/**
 * Create a status table
 */
export function createStatusTable(): Table.Table {
  return new Table({
    chars: { mid: '', 'left-mid': '', 'mid-mid': '', 'right-mid': '' },
    style: { 'padding-left': 0, 'padding-right': 2 },
  });
}

/**
 * Print a section header
 */
export function printSection(title: string): void {
  console.log();
  console.log(chalk.bold(title));
}

/**
 * Print success message
 */
export function printSuccess(message: string): void {
  console.log(chalk.green('✓'), message);
}

/**
 * Print error message
 */
export function printError(message: string): void {
  console.log(chalk.red('✗'), message);
}

/**
 * Print warning message
 */
export function printWarning(message: string): void {
  console.log(chalk.yellow('!'), message);
}

/**
 * Print info message
 */
export function printInfo(message: string): void {
  console.log(chalk.blue('i'), message);
}

/**
 * Format namespace number to name
 */
export function formatNamespace(ns: number): string {
  const names: Record<number, string> = {
    0: 'Main',
    6: 'File',
    8: 'MediaWiki',
    10: 'Template',
    14: 'Category',
    828: 'Module',
    3000: 'Goldenlight',
  };
  return names[ns] || `NS${ns}`;
}

/**
 * Format sync status
 */
export function formatSyncStatus(status: string): string {
  const colors: Record<string, (s: string) => string> = {
    synced: chalk.green,
    local_modified: chalk.yellow,
    wiki_modified: chalk.blue,
    conflict: chalk.red,
    staged: chalk.cyan,
    new: chalk.magenta,
  };
  const color = colors[status] || chalk.white;
  return color(status);
}
