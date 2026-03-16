import { parseSource } from '../parser/chunker.js';
import { openDatabase, getMetaValue } from '../store/db.js';
import { queryKNN, getAllFunctions } from '../store/index-store.js';
import { createClient, preflight, detectModel } from '../embedder/ollama-client.js';
import { embedQuery } from '../embedder/embed.js';
import { loadIgnoreFile, applyExclusions } from '../ignore/ignore-file.js';
import { detectStaleExclusions } from '../ignore/stale-detector.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import type { Candidate, QueryFunction, QueryResult } from '../output/types.js';

export interface QueryOptions {
  source: string;
  fileName?: string;
  topK?: number;
  threshold?: number;
  dbPath?: string;
  projectRoot?: string;
}

export async function runQuery(options: QueryOptions): Promise<QueryResult> {
  const start = Date.now();
  const topK = options.topK ?? 5;
  const threshold = options.threshold ?? 0.3;
  const projectRoot = options.projectRoot ?? findProjectRoot(process.cwd());
  const dbPath = options.dbPath ?? resolveDbPath(projectRoot);
  const fileName = options.fileName ?? '<input>';

  // Parse input into function chunks
  const parsed = parseSource(options.source, fileName);
  if (parsed.chunks.length === 0) {
    return {
      query_functions: [],
      warnings: ['No functions found in input.'],
      meta: {
        model: 'unknown',
        indexed_functions: 0,
        query_functions: 0,
        elapsed_ms: Date.now() - start,
      },
    };
  }

  // Open database
  const db = openDatabase(dbPath);
  const allFunctions = getAllFunctions(db);

  // Check Ollama
  const client = createClient();
  const check = await preflight(client);
  if (!check.ok) {
    db.close();
    throw new Error(check.message);
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
    const queryEmbedding = await embedQuery(client, model.name, chunk.sourceText);
    const knnResults = queryKNN(db, queryEmbedding, topK * 2, threshold);

    let candidates: Candidate[] = knnResults.map((r) => ({
      name: r.functionName,
      path: r.filePath,
      line: r.startLine,
      similarity: r.distance,
      detectionMethod: 'embedding' as const,
      source: r.sourceText,
      signatureHash: r.signatureHash,
    }));

    candidates = applyExclusions(
      ignoreFile,
      candidates,
      chunk.signatureHash,
    ) as Candidate[];

    candidates = candidates.slice(0, topK);

    queryFunctions.push({
      name: chunk.functionName,
      file: fileName,
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
  return result;
}
