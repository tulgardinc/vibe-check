import type { Command } from 'commander';
import fs from 'node:fs';
import { openDatabase, getMetaValue } from '../store/db.js';
import { getAllFunctions } from '../store/index-store.js';
import { getTrackedFiles } from '../store/file-tracker.js';
import { loadIgnoreFile } from '../ignore/ignore-file.js';
import { detectStaleExclusions } from '../ignore/stale-detector.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { error } from '../util/logger.js';

interface StatusOptions {
  db?: string;
}

export function registerStatusCommand(program: Command): void {
  program
    .command('status')
    .description('Show index health and statistics')
    .option('--db <path>', 'Path to database file', '.codeuse.db')
    .action(async (options: StatusOptions) => {
      try {
        const projectRoot = findProjectRoot(process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        if (!fs.existsSync(dbPath)) {
          console.log('No index found. Run `codeuse index` to create one.');
          return;
        }

        const db = openDatabase(dbPath);
        const functions = getAllFunctions(db);
        const trackedFiles = getTrackedFiles(db);
        const ignoreFile = loadIgnoreFile(projectRoot);
        const staleWarnings = detectStaleExclusions(db, ignoreFile);

        const modelName = getMetaValue(db, 'model_name') ?? 'unknown';
        const dimensions = getMetaValue(db, 'model_dimensions') ?? 'unknown';
        const lastIndexed = getMetaValue(db, 'last_indexed_at') ?? 'never';

        // Get DB file size
        const stat = fs.statSync(dbPath);
        const sizeMb = (stat.size / 1024 / 1024).toFixed(1);

        const embedded = functions.filter((f) => f.embedding !== null).length;
        const unembedded = functions.length - embedded;

        db.close();

        console.log(`codeuse index status`);
        console.log(`  Database:           ${dbPath} (${sizeMb} MB)`);
        console.log(`  Model:              ${modelName}`);
        console.log(`  Dimensions:         ${dimensions}`);
        console.log(`  Indexed functions:  ${functions.length}${unembedded > 0 ? ` (${unembedded} awaiting embedding)` : ''}`);
        console.log(`  Tracked files:      ${trackedFiles.size}`);
        console.log(`  Last indexed:       ${lastIndexed}`);
        console.log(`  Exclusions:         ${ignoreFile.exclusions.length}${staleWarnings.length > 0 ? ` (${staleWarnings.length} stale)` : ''}`);
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
