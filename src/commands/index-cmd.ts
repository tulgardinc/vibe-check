import type { Command } from 'commander';
import { parseFile } from '../parser/chunker.js';
import { openDatabase, getMetaValue, setMetaValue } from '../store/db.js';
import { upsertFunctions, deleteFunctionsForFile, getFunctionsWithoutEmbeddings, updateEmbedding } from '../store/index-store.js';
import { computeChangedFiles, upsertTrackedFile, removeTrackedFile } from '../store/file-tracker.js';
import { createClient, checkHealth, detectModel } from '../embedder/ollama-client.js';
import { embedChunks } from '../embedder/embed.js';
import { contentHash } from '../util/hash.js';
import { findProjectRoot, resolveDbPath, findTypeScriptFiles } from '../util/config.js';
import { info, verbose, warn, error, setLogLevel } from '../util/logger.js';
import fs from 'node:fs/promises';

interface IndexOptions {
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
    .action(async (inputPath: string | undefined, options: IndexOptions) => {
      if (options.verbose) setLogLevel('verbose');

      try {
        const projectRoot = findProjectRoot(inputPath ?? process.cwd());
        const scanPath = inputPath ?? projectRoot;
        const dbPath = resolveDbPath(projectRoot, options.db);

        info(`Project root: ${projectRoot}`);
        verbose(`Database: ${dbPath}`);

        // Find TypeScript files
        const files = await findTypeScriptFiles(scanPath);
        info(`Found ${files.length} TypeScript files`);

        if (options.dryRun) {
          for (const f of files) info(`  ${f}`);
          return;
        }

        // Check Ollama
        const client = createClient();
        if (!(await checkHealth(client))) {
          error('Cannot connect to Ollama. Is it running? Try: ollama serve');
          process.exit(1);
        }

        const model = await detectModel(client);
        info(`Using model: ${model.name} (${model.tier}, ${model.dimensions}d)`);

        // Open database
        const db = openDatabase(dbPath);

        // Check model mismatch
        const storedModel = getMetaValue(db, 'model_name');
        if (storedModel && storedModel !== model.name && !options.force) {
          error(
            `Index was built with "${storedModel}" but current model is "${model.name}". Use --force to re-index.`,
          );
          process.exit(1);
        }

        // Compute changes
        const changes = options.force
          ? { added: files, modified: [], deleted: [], unchanged: [] }
          : await computeChangedFiles(db, files);

        info(
          `Changes: ${changes.added.length} added, ${changes.modified.length} modified, ${changes.deleted.length} deleted, ${changes.unchanged.length} unchanged`,
        );

        // Handle deletions
        for (const fp of changes.deleted) {
          verbose(`Removing: ${fp}`);
          deleteFunctionsForFile(db, fp);
          removeTrackedFile(db, fp);
        }

        // Parse and upsert added/modified files
        const filesToProcess = [...changes.added, ...changes.modified];
        let totalChunks = 0;

        for (const fp of filesToProcess) {
          verbose(`Parsing: ${fp}`);

          // Remove old functions for modified files
          if (changes.modified.includes(fp)) {
            deleteFunctionsForFile(db, fp);
          }

          const parsed = await parseFile(fp);

          for (const err of parsed.parseErrors) {
            warn(`${fp}: ${err}`);
          }

          if (parsed.chunks.length > 0) {
            upsertFunctions(db, parsed.chunks);
            totalChunks += parsed.chunks.length;
          }

          // Update file tracker
          const source = await fs.readFile(fp, 'utf-8');
          const stat = await fs.stat(fp);
          upsertTrackedFile(db, fp, contentHash(source), stat.mtimeMs);
        }

        info(`Parsed ${totalChunks} functions from ${filesToProcess.length} files`);

        // Embed functions without embeddings
        const unembedded = getFunctionsWithoutEmbeddings(db);
        if (unembedded.length > 0) {
          info(`Embedding ${unembedded.length} functions...`);

          // Convert StoredFunctions back to minimal FunctionChunk shape for embedChunks
          const chunksForEmbedding = unembedded.map((f) => ({
            id: f.id,
            filePath: f.filePath,
            functionName: f.functionName,
            sourceText: f.sourceText,
            startLine: f.startLine,
            endLine: f.endLine,
            params: JSON.parse(f.paramsJson),
            returnType: f.returnType,
            isExported: f.isExported,
            signatureHash: f.signatureHash,
          }));

          const embeddings = await embedChunks(
            client,
            model.name,
            chunksForEmbedding,
            (done, total) => verbose(`  Embedded ${done}/${total}`),
          );

          for (const e of embeddings) {
            updateEmbedding(db, e.functionId, e.embedding);
          }
        }

        // Update metadata
        setMetaValue(db, 'model_name', model.name);
        setMetaValue(db, 'model_dimensions', String(model.dimensions));
        setMetaValue(db, 'last_indexed_at', new Date().toISOString());
        if (!getMetaValue(db, 'created_at')) {
          setMetaValue(db, 'created_at', new Date().toISOString());
        }

        db.close();
        info('Indexing complete.');
      } catch (e) {
        error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
    });
}
