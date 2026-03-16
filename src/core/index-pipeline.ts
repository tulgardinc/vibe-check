import fs from 'node:fs/promises';
import { parseFile } from '../parser/chunker.js';
import { openDatabase, getMetaValue, setMetaValue } from '../store/db.js';
import { upsertFunctions, deleteFunctionsForFile, getFunctionsWithoutEmbeddings, updateEmbedding } from '../store/index-store.js';
import { computeChangedFiles, upsertTrackedFile, removeTrackedFile } from '../store/file-tracker.js';
import { createClient, preflight } from '../embedder/ollama-client.js';
import { contentHash } from '../util/hash.js';
import { findProjectRoot, resolveDbPath, findTypeScriptFiles } from '../util/config.js';

export interface IndexOptions {
  path?: string;
  dbPath?: string;
  force?: boolean;
  projectRoot?: string;
  onProgress?: (message: string) => void;
}

export interface IndexResult {
  filesScanned: number;
  functionsIndexed: number;
  added: number;
  modified: number;
  deleted: number;
  unchanged: number;
  model: string;
  tier: string;
  dimensions: number;
}

export async function runIndex(options: IndexOptions): Promise<IndexResult> {
  const log = options.onProgress ?? (() => {});
  const projectRoot = options.projectRoot ?? findProjectRoot(options.path ?? process.cwd());
  const scanPath = options.path ?? projectRoot;
  const dbPath = options.dbPath ?? resolveDbPath(projectRoot);

  log(`Project root: ${projectRoot}`);

  // Find TypeScript files
  const files = await findTypeScriptFiles(scanPath);
  log(`Found ${files.length} TypeScript files`);

  // Check Ollama and get embedder
  const client = createClient();
  const check = await preflight(client);
  if (!check.ok) {
    throw new Error(check.message);
  }
  const { embedder } = check;
  log(check.message);

  // Open database
  const db = openDatabase(dbPath);
  try {
    // Check model mismatch
    const storedModel = getMetaValue(db, 'model_name');
    if (storedModel && storedModel !== embedder.modelName && !options.force) {
      throw new Error(
        `Index was built with "${storedModel}" but current model is "${embedder.modelName}". Use force option to re-index.`,
      );
    }

    // Compute changes
    const changes = options.force
      ? { added: files, modified: [], deleted: [], unchanged: [] }
      : await computeChangedFiles(db, files);

    log(
      `Changes: ${changes.added.length} added, ${changes.modified.length} modified, ${changes.deleted.length} deleted, ${changes.unchanged.length} unchanged`,
    );

    // Handle deletions
    for (const fp of changes.deleted) {
      deleteFunctionsForFile(db, fp);
      removeTrackedFile(db, fp);
    }

    // Parse and upsert added/modified files
    const filesToProcess = [...changes.added, ...changes.modified];
    let totalChunks = 0;

    for (const fp of filesToProcess) {
      if (changes.modified.includes(fp)) {
        deleteFunctionsForFile(db, fp);
      }

      // Upsert tracked file first (foreign key: functions → tracked_files)
      const source = await fs.readFile(fp, 'utf-8');
      const stat = await fs.stat(fp);
      upsertTrackedFile(db, fp, contentHash(source), stat.mtimeMs);

      const parsed = await parseFile(fp);

      if (parsed.chunks.length > 0) {
        upsertFunctions(db, parsed.chunks);
        totalChunks += parsed.chunks.length;
      }
    }

    log(`Parsed ${totalChunks} functions from ${filesToProcess.length} files`);

    // Embed functions without embeddings
    const unembedded = getFunctionsWithoutEmbeddings(db);
    if (unembedded.length > 0) {
      log(`Embedding ${unembedded.length} functions...`);

      const inputs = unembedded.map((f) => f.sourceText);
      const embeddings = await embedder.embedBatch(
        inputs,
        (done, total) => log(`Embedded ${done}/${total}`),
      );

      for (let i = 0; i < unembedded.length; i++) {
        updateEmbedding(db, unembedded[i].id, embeddings[i]);
      }
    }

    // Update metadata
    setMetaValue(db, 'model_name', embedder.modelName);
    setMetaValue(db, 'model_dimensions', String(embedder.dimensions));
    setMetaValue(db, 'last_indexed_at', new Date().toISOString());
    if (!getMetaValue(db, 'created_at')) {
      setMetaValue(db, 'created_at', new Date().toISOString());
    }

    return {
      filesScanned: filesToProcess.length,
      functionsIndexed: totalChunks,
      added: changes.added.length,
      modified: changes.modified.length,
      deleted: changes.deleted.length,
      unchanged: changes.unchanged.length,
      model: embedder.modelName,
      tier: embedder.tier,
      dimensions: embedder.dimensions,
    };
  } finally {
    db.close();
  }
}
