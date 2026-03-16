import fs from 'node:fs';
import { openDatabase, getMetaValue } from '../store/db.js';
import { getAllFunctions } from '../store/index-store.js';
import { getTrackedFiles } from '../store/file-tracker.js';
import { loadIgnoreFile } from '../ignore/ignore-file.js';
import { detectStaleExclusions } from '../ignore/stale-detector.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';

export interface StatusResult {
  exists: boolean;
  dbPath: string;
  sizeMb: string;
  model: string;
  dimensions: string;
  indexedFunctions: number;
  unembedded: number;
  trackedFiles: number;
  lastIndexed: string;
  exclusions: number;
  staleExclusions: number;
}

export function runStatus(options?: {
  dbPath?: string;
  projectRoot?: string;
}): StatusResult {
  const projectRoot = options?.projectRoot ?? findProjectRoot(process.cwd());
  const dbPath = options?.dbPath ?? resolveDbPath(projectRoot);

  if (!fs.existsSync(dbPath)) {
    return {
      exists: false,
      dbPath,
      sizeMb: '0',
      model: 'unknown',
      dimensions: 'unknown',
      indexedFunctions: 0,
      unembedded: 0,
      trackedFiles: 0,
      lastIndexed: 'never',
      exclusions: 0,
      staleExclusions: 0,
    };
  }

  const db = openDatabase(dbPath);
  try {
    const functions = getAllFunctions(db);
    const trackedFiles = getTrackedFiles(db);
    const ignoreFile = loadIgnoreFile(projectRoot);
    const staleWarnings = detectStaleExclusions(db, ignoreFile);

    const model = getMetaValue(db, 'model_name') ?? 'unknown';
    const dimensions = getMetaValue(db, 'model_dimensions') ?? 'unknown';
    const lastIndexed = getMetaValue(db, 'last_indexed_at') ?? 'never';

    const stat = fs.statSync(dbPath);
    const sizeMb = (stat.size / 1024 / 1024).toFixed(1);

    const embedded = functions.filter((f) => f.embedding !== null).length;

    return {
      exists: true,
      dbPath,
      sizeMb,
      model,
      dimensions,
      indexedFunctions: functions.length,
      unembedded: functions.length - embedded,
      trackedFiles: trackedFiles.size,
      lastIndexed,
      exclusions: ignoreFile.exclusions.length,
      staleExclusions: staleWarnings.length,
    };
  } finally {
    db.close();
  }
}
