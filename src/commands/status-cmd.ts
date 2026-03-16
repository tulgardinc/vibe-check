import type { Command } from 'commander';
import { runStatus } from '../core/status-pipeline.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { error } from '../util/logger.js';

interface CliStatusOptions {
  db?: string;
}

export function registerStatusCommand(program: Command): void {
  program
    .command('status')
    .description('Show index health and statistics')
    .option('--db <path>', 'Path to database file', '.codeuse.db')
    .action(async (options: CliStatusOptions) => {
      try {
        const projectRoot = findProjectRoot(process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        const result = runStatus({ dbPath, projectRoot });

        if (!result.exists) {
          console.log('No index found. Run `codeuse index` to create one.');
          return;
        }

        console.log(`codeuse index status`);
        console.log(`  Database:           ${result.dbPath} (${result.sizeMb} MB)`);
        console.log(`  Model:              ${result.model}`);
        console.log(`  Dimensions:         ${result.dimensions}`);
        console.log(`  Indexed functions:  ${result.indexedFunctions}${result.unembedded > 0 ? ` (${result.unembedded} awaiting embedding)` : ''}`);
        console.log(`  Tracked files:      ${result.trackedFiles}`);
        console.log(`  Last indexed:       ${result.lastIndexed}`);
        console.log(`  Exclusions:         ${result.exclusions}${result.staleExclusions > 0 ? ` (${result.staleExclusions} stale)` : ''}`);
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
