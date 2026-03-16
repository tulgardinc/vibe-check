import type { Command } from 'commander';
import { runIndex } from '../core/index-pipeline.js';
import { findProjectRoot, resolveDbPath, findTypeScriptFiles } from '../util/config.js';
import { info, verbose, error, setLogLevel } from '../util/logger.js';

interface CliIndexOptions {
  db?: string;
  force?: boolean;
  verbose?: boolean;
  dryRun?: boolean;
}

export function registerIndexCommand(program: Command): void {
  program
    .command('index [path]')
    .description('Index TypeScript functions in the codebase')
    .option('--db <path>', 'Path to database file', '.codeuse.db')
    .option('--force', 'Force full re-index')
    .option('--verbose', 'Show detailed progress')
    .option('--dry-run', 'Show what would be indexed without writing')
    .action(async (inputPath: string | undefined, options: CliIndexOptions) => {
      if (options.verbose) setLogLevel('verbose');

      try {
        const projectRoot = findProjectRoot(inputPath ?? process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        if (options.dryRun) {
          const files = await findTypeScriptFiles(inputPath ?? projectRoot);
          info(`Found ${files.length} TypeScript files:`);
          for (const f of files) info(`  ${f}`);
          return;
        }

        const result = await runIndex({
          path: inputPath,
          dbPath,
          force: options.force,
          projectRoot,
          onProgress: options.verbose ? verbose : info,
        });

        info(
          `Indexed ${result.functionsIndexed} functions from ${result.filesScanned} files ` +
          `(${result.added} added, ${result.modified} modified, ${result.deleted} deleted) ` +
          `using ${result.model} (${result.tier})`,
        );
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
