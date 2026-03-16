import type { Command } from 'commander';
import fs from 'node:fs';
import { runQuery } from '../core/query-pipeline.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { error, setLogLevel } from '../util/logger.js';
import { formatJson, formatHuman } from '../output/formatter.js';

interface CliQueryOptions {
  stdin?: boolean;
  topK?: string;
  threshold?: string;
  db?: string;
  prefilter?: boolean;
  json?: boolean;
  verbose?: boolean;
}

export function registerQueryCommand(program: Command): void {
  program
    .command('query <file>')
    .description('Find existing functions similar to new code')
    .option('--stdin', 'Read from stdin instead of file')
    .option('--top-k <n>', 'Number of candidates per function', '5')
    .option('--threshold <n>', 'Cosine distance threshold', '0.3')
    .option('--db <path>', 'Path to database file', '.codeuse.db')
    .option('--no-prefilter', 'Skip jscpd pre-filtering')
    .option('--json', 'Force JSON output')
    .option('--verbose', 'Show detailed matching info')
    .action(async (file: string, options: CliQueryOptions) => {
      if (options.verbose) setLogLevel('verbose');

      try {
        const projectRoot = findProjectRoot(process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        let source: string;
        if (options.stdin) {
          source = fs.readFileSync(0, 'utf-8');
        } else {
          source = fs.readFileSync(file, 'utf-8');
        }

        const result = await runQuery({
          source,
          fileName: file,
          topK: parseInt(options.topK ?? '5', 10),
          threshold: parseFloat(options.threshold ?? '0.3'),
          dbPath,
          projectRoot,
        });

        const useJson = options.json || !process.stdout.isTTY;
        console.log(useJson ? formatJson(result) : formatHuman(result));
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
