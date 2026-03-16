import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_EXCLUDES = ['node_modules', 'dist', '.git', 'coverage', '.next', 'build', 'experiment'];

export function findProjectRoot(startDir: string): string {
  let dir = path.resolve(startDir);
  while (true) {
    if (
      fs.existsSync(path.join(dir, '.git')) ||
      fs.existsSync(path.join(dir, 'package.json'))
    ) {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      // Reached filesystem root, use startDir
      return path.resolve(startDir);
    }
    dir = parent;
  }
}

export function getDefaultExcludes(): string[] {
  return DEFAULT_EXCLUDES;
}

export function resolveDbPath(projectRoot: string, overridePath?: string): string {
  if (overridePath) {
    return path.resolve(overridePath);
  }
  return path.join(projectRoot, '.codeuse.db');
}

export async function findTypeScriptFiles(
  rootDir: string,
  excludes: string[] = DEFAULT_EXCLUDES,
): Promise<string[]> {
  const { readdir } = await import('node:fs/promises');

  const entries = await readdir(rootDir, { recursive: true, withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    if (!entry.isFile()) continue;
    if (!entry.name.endsWith('.ts')) continue;
    if (entry.name.endsWith('.d.ts')) continue;
    if (entry.name.endsWith('.test.ts')) continue;
    if (entry.name.endsWith('.spec.ts')) continue;

    // parentPath was added in Node 20.12; fall back to the older `path` property
    const parentDir: string = entry.parentPath ?? (entry as unknown as { path: string }).path;
    const relativePath = path.relative(rootDir, path.join(parentDir, entry.name));

    const parts = relativePath.split(path.sep);
    if (parts.some((p) => excludes.includes(p))) continue;

    files.push(path.join(rootDir, relativePath));
  }

  return files.sort();
}
