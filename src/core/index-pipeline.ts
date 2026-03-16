import fs from 'node:fs/promises';
import { parseFile } from '../parser/chunker.js';
import { openDatabase, getMetaValue, setMetaValue } from '../store/db.js';
import { upsertFunctions, deleteFunctionsForFile, getFunctionsWithoutEmbeddings, updateEmbedding } from '../store/index-store.js';
import { computeChangedFiles, upsertTrackedFile, removeTrackedFile } from '../store/file-tracker.js';
import { createClient, preflight } from '../embedder/ollama-client.js';
import { embedChunks } from '../embedder/embed.js';
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

  // Check Ollama
  const client = createClient();
  const check = await preflight(client);
  if (!check.ok) {
    throw new Error(check.message);
  }
  log(check.message);

  // Re-detect model (preflight confirmed it exists)
  const { detectModel } = await import('../embedder/ollama-client.js');
  const model = await detectModel(client);

  // Open database
  const db = openDatabase(dbPath);

  // Check model mismatch
  const storedModel = getMetaValue(db, 'model_name');
  if (storedModel && storedModel !== model.name && !options.force) {
    db.close();
    throw new Error(
      `Index was built with "${storedModel}" but current model is "${model.name}". Use force option to re-index.`,
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
      (done, total) => log(`Embedded ${done}/${total}`),
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

  return {
    filesScanned: filesToProcess.length,
    functionsIndexed: totalChunks,
    added: changes.added.length,
    modified: changes.modified.length,
    deleted: changes.deleted.length,
    unchanged: changes.unchanged.length,
    model: model.name,
    tier: model.tier,
    dimensions: model.dimensions,
  };
}
