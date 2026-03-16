import fs from 'node:fs/promises';
import type Database from 'better-sqlite3';
import { contentHash as computeContentHash } from '../util/hash.js';
import type { FileRecord } from './types.js';

export function getTrackedFiles(db: Database.Database): Map<string, FileRecord> {
  const rows = db.prepare('SELECT * FROM tracked_files').all() as Array<{
    file_path: string;
    content_hash: string;
    mtime_ms: number;
    indexed_at: string;
  }>;

  const map = new Map<string, FileRecord>();
  for (const r of rows) {
    map.set(r.file_path, {
      filePath: r.file_path,
      contentHash: r.content_hash,
      mtimeMs: r.mtime_ms,
      indexedAt: r.indexed_at,
    });
  }
  return map;
}

export function upsertTrackedFile(
  db: Database.Database,
  filePath: string,
  hash: string,
  mtimeMs: number,
): void {
  db.prepare(
    `INSERT OR REPLACE INTO tracked_files (file_path, content_hash, mtime_ms, indexed_at)
     VALUES (?, ?, ?, ?)`,
  ).run(filePath, hash, mtimeMs, new Date().toISOString());
}

export function removeTrackedFile(
  db: Database.Database,
  filePath: string,
): void {
  db.prepare('DELETE FROM tracked_files WHERE file_path = ?').run(filePath);
}

export interface FileChanges {
  added: string[];
  modified: string[];
  deleted: string[];
  unchanged: string[];
}

export async function computeChangedFiles(
  db: Database.Database,
  filePaths: string[],
): Promise<FileChanges> {
  const tracked = getTrackedFiles(db);
  const currentPaths = new Set(filePaths);

  const added: string[] = [];
  const modified: string[] = [];
  const deleted: string[] = [];
  const unchanged: string[] = [];

  // Check for added and modified files
  for (const fp of filePaths) {
    const existing = tracked.get(fp);
    if (!existing) {
      added.push(fp);
      continue;
    }

    // Fast path: check mtime first
    const stat = await fs.stat(fp);
    if (stat.mtimeMs === existing.mtimeMs) {
      unchanged.push(fp);
      continue;
    }

    // mtime changed — check content hash
    const source = await fs.readFile(fp, 'utf-8');
    const hash = computeContentHash(source);
    if (hash === existing.contentHash) {
      unchanged.push(fp);
    } else {
      modified.push(fp);
    }
  }

  // Check for deleted files
  for (const [fp] of tracked) {
    if (!currentPaths.has(fp)) {
      deleted.push(fp);
    }
  }

  return { added, modified, deleted, unchanged };
}
