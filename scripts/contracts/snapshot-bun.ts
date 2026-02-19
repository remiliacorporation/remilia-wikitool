import { resolve } from 'node:path';
import { createFilesystem } from '../../packages/core/src/storage/filesystem.ts';

export interface SnapshotFile {
  relative_path: string;
  title: string;
  namespace: string;
  is_redirect: boolean;
  redirect_target: string | null;
  content_hash: string;
}

export interface RuntimeSnapshot {
  runtime: 'bun' | 'rust';
  fixture_root: string;
  content_file_count: number;
  template_file_count: number;
  files: SnapshotFile[];
}

interface SnapshotOptions {
  projectRoot: string;
  contentDir: string;
  templatesDir: string;
}

export function generateBunSnapshot(options: SnapshotOptions): RuntimeSnapshot {
  const root = resolve(options.projectRoot);
  const filesystem = createFilesystem(root, {
    contentDir: options.contentDir,
    templatesDir: options.templatesDir,
  });

  const contentFiles = filesystem.scanContentFiles();
  const templateFiles = filesystem.scanTemplateFiles();
  const files = [...contentFiles, ...templateFiles]
    .map((file) => ({
      relative_path: normalize(file.filepath),
      title: file.title,
      namespace: namespaceFromTitle(file.title),
      is_redirect: file.isRedirect,
      redirect_target: file.redirectTarget,
      content_hash: file.contentHash,
    }))
    .sort((left, right) => left.relative_path.localeCompare(right.relative_path));

  return {
    runtime: 'bun',
    fixture_root: normalize(root),
    content_file_count: contentFiles.length,
    template_file_count: templateFiles.length,
    files,
  };
}

function namespaceFromTitle(title: string): string {
  if (title.startsWith('Category:')) return 'Category';
  if (title.startsWith('Template:')) return 'Template';
  if (title.startsWith('Module:')) return 'Module';
  if (title.startsWith('MediaWiki:')) return 'MediaWiki';
  if (title.startsWith('File:')) return 'File';
  if (title.startsWith('User:')) return 'User';
  if (title.startsWith('Goldenlight:')) return 'Goldenlight';
  return 'Main';
}

function normalize(path: string): string {
  return path.replace(/\\/g, '/');
}

function getArg(flag: string, fallback: string): string {
  const args = process.argv.slice(2);
  const index = args.indexOf(flag);
  if (index === -1 || index + 1 >= args.length) {
    return fallback;
  }
  return args[index + 1];
}

if (import.meta.main) {
  const snapshot = generateBunSnapshot({
    projectRoot: getArg('--project-root', process.cwd()),
    contentDir: getArg('--content-dir', 'wiki_content'),
    templatesDir: getArg('--templates-dir', 'custom/templates'),
  });
  console.log(JSON.stringify(snapshot, null, 2));
}
