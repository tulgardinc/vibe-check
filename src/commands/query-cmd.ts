import type { Command } from 'commander';
import fs from 'node:fs';
import { parseSource } from '../parser/chunker.js';
import { openDatabase, getMetaValue } from '../store/db.js';
import { queryKNN, getAllFunctions } from '../store/index-store.js';
import { createClient, checkHealth, detectModel } from '../embedder/ollama-client.js';
import { embedQuery } from '../embedder/embed.js';
import { loadIgnoreFile, applyExclusions } from '../ignore/ignore-file.js';
import { detectStaleExclusions } from '../ignore/stale-detector.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { error, setLogLevel } from '../util/logger.js';
import { formatJson, formatHuman } from '../output/formatter.js';
import type { Candidate, QueryFunction, QueryResult } from '../output/types.js';

interface QueryOptions {
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
    .action(async (file: string, options: QueryOptions) => {
      if (options.verbose) setLogLevel('verbose');

      try {
        const start = Date.now();
        const topK = parseInt(options.topK ?? '5', 10);
        const threshold = parseFloat(options.threshold ?? '0.3');
        const projectRoot = findProjectRoot(process.cwd());
        const dbPath = resolveDbPath(projectRoot, options.db);

        // Read input
        let source: string;
        if (options.stdin) {
          source = fs.readFileSync(0, 'utf-8');
        } else {
          source = fs.readFileSync(file, 'utf-8');
        }

        // Parse input into function chunks
        const parsed = parseSource(source, file);
        if (parsed.chunks.length === 0) {
          error('No functions found in input.');
          process.exit(1);
        }

        // Open database
        const db = openDatabase(dbPath);
        const allFunctions = getAllFunctions(db);

        // Check Ollama
        const client = createClient();
        if (!(await checkHealth(client))) {
          error('Cannot connect to Ollama. Is it running? Try: ollama serve');
          process.exit(1);
        }

        const model = await detectModel(client);
        const storedModel = getMetaValue(db, 'model_name');
        const warnings: string[] = [];

        if (storedModel && storedModel !== model.name) {
          warnings.push(
            `Index was built with "${storedModel}" but current model is "${model.name}". Results may be inaccurate.`,
          );
        }

        // Load ignore file
        const ignoreFile = loadIgnoreFile(projectRoot);

        // Detect stale exclusions
        const staleWarnings = detectStaleExclusions(db, ignoreFile);
        for (const sw of staleWarnings) {
          warnings.push(sw.reason);
        }

        // Process each query function
        const queryFunctions: QueryFunction[] = [];

        for (const chunk of parsed.chunks) {
          // Embed the query function
          const queryEmbedding = await embedQuery(client, model.name, chunk.sourceText);

          // KNN search
          const knnResults = queryKNN(db, queryEmbedding, topK * 2, threshold);

          // Convert to candidates
          let candidates: Candidate[] = knnResults.map((r) => ({
            name: r.functionName,
            path: r.filePath,
            line: r.startLine,
            similarity: r.distance,
            detectionMethod: 'embedding' as const,
            source: r.sourceText,
            signatureHash: r.signatureHash,
          }));

          // Apply exclusions
          candidates = applyExclusions(
            ignoreFile,
            candidates,
            chunk.signatureHash,
          ) as Candidate[];

          // Trim to topK
          candidates = candidates.slice(0, topK);

          queryFunctions.push({
            name: chunk.functionName,
            file,
            line: chunk.startLine,
            candidates,
          });
        }

        const result: QueryResult = {
          query_functions: queryFunctions,
          warnings,
          meta: {
            model: model.name,
            indexed_functions: allFunctions.length,
            query_functions: parsed.chunks.length,
            elapsed_ms: Date.now() - start,
          },
        };

        db.close();

        const useJson = options.json || !process.stdout.isTTY;
        console.log(useJson ? formatJson(result) : formatHuman(result));
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
