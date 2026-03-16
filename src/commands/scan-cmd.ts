import type { Command } from 'commander';
import { runScan } from '../core/scan-pipeline.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { info, verbose, error, setLogLevel } from '../util/logger.js';
import { formatScanJson, formatScanHuman } from '../output/scan-formatter.js';

interface CliScanOptions {
  topN?: string;
  threshold?: string;
  db?: string;
  json?: boolean;
  verbose?: boolean;
}

export function registerScanCommand(program: Command): void {
  program
    .command('scan')
    .description('Scan the entire indexed codebase for similar function pairs')
    .option('--top-n <n>', 'Maximum number of pairs to show', '50')
    .option('--threshold <n>', 'Cosine distance threshold (lower = stricter)', '0.25')
    .option('--db <path>', 'Path to database file', '.codeuse.db')
    .option('--json', 'Force JSON output')
    .option('--verbose', 'Show progress')
    .action(async (options: CliScanOptions) => {
      if (options.verbose) setLogLevel('verbose');

      try {
        const projectRoot = findProjectRoot(process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        const result = runScan({
          topN: parseInt(options.topN ?? '50', 10),
          threshold: parseFloat(options.threshold ?? '0.25'),
          dbPath,
          projectRoot,
          onProgress: options.verbose ? verbose : info,
        });

        const useJson = options.json || !process.stdout.isTTY;
        console.log(useJson ? formatScanJson(result) : formatScanHuman(result, projectRoot));
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
