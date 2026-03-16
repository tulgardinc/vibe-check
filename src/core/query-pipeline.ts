import fs from 'node:fs';
import path from 'node:path';
import { parseSource } from '../parser/chunker.js';
import { openDatabase, getMetaValue } from '../store/db.js';
import { queryKNN, countFunctions } from '../store/index-store.js';
import { createClient, preflight } from '../embedder/ollama-client.js';
import { loadIgnoreFile, applyExclusions } from '../ignore/ignore-file.js';
import { detectStaleExclusions } from '../ignore/stale-detector.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import { rerankCandidates } from '../ranking/jaccard.js';
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
  const rawFileName = options.fileName ?? '<input>';
  // Resolve to absolute path so chunk IDs match indexed function IDs
  const fileName = rawFileName === '<input>' ? rawFileName : path.resolve(rawFileName);

  // Parse input into function chunks
  const parsed = parseSource(options.source, fileName);
  if (parsed.chunks.length === 0) {
    return {
      queryFunctions: [],
      warnings: ['No functions found in input.'],
      meta: {
        model: 'unknown',
        indexedFunctions: 0,
        queryFunctions: 0,
        elapsedMs: Date.now() - start,
      },
    };
  }

  // Open database — don't create an empty one if it doesn't exist
  if (!fs.existsSync(dbPath)) {
    throw new Error(
      `No index found at ${dbPath}. Run "codeuse index" in a TypeScript project first.`,
    );
  }
  const db = openDatabase(dbPath);
  try {
    const indexedCount = countFunctions(db);

    // Check Ollama and get embedder
    const client = createClient();
    const check = await preflight(client);
    if (!check.ok) {
      throw new Error(check.message);
    }
    const { embedder } = check;

    const storedModel = getMetaValue(db, 'model_name');
    const warnings: string[] = [];

    if (storedModel && storedModel !== embedder.modelName) {
      warnings.push(
        `Index was built with "${storedModel}" but current model is "${embedder.modelName}". Results may be inaccurate.`,
      );
    }

    // Load ignore file
    const ignoreFile = loadIgnoreFile(projectRoot);

    // Detect stale exclusions
    const staleWarnings = detectStaleExclusions(db, ignoreFile);
    for (const sw of staleWarnings) {
      warnings.push(sw.reason);
    }

    // Batch-embed all query functions at once
    const queryTexts = parsed.chunks.map((c) => c.sourceText);
    const queryEmbeddings = await embedder.embedBatch(queryTexts);

    // Process each query function
    const queryFunctions: QueryFunction[] = [];

    for (let i = 0; i < parsed.chunks.length; i++) {
      const chunk = parsed.chunks[i];
      const queryEmbedding = queryEmbeddings[i];

      // Over-fetch to give Jaccard re-ranking room to reorder
      const knnResults = queryKNN(db, queryEmbedding, topK * 3, threshold);

      const rawCandidates = knnResults
        // Filter self-matches (when querying an already-indexed file)
        .filter((r) => r.id !== chunk.id)
        .map((r) => ({
          name: r.functionName,
          path: r.filePath,
          line: r.startLine,
          distance: r.distance,
          detectionMethod: 'embedding' as const,
          source: r.sourceText,
          signatureHash: r.signatureHash,
          chunkType: r.chunkType,
          context: r.context,
        }));

      // Jaccard re-rank
      const reranked = rerankCandidates(chunk.sourceText, rawCandidates, 0.7);

      let candidates: Candidate[] = reranked.map((r) => ({
        name: r.name,
        path: r.path,
        line: r.line,
        distance: r.combinedScore,
        detectionMethod: r.detectionMethod,
        source: r.source,
        signatureHash: r.signatureHash,
        chunkType: r.chunkType,
        context: r.context,
        jaccardSimilarity: r.jaccardSimilarity,
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
        chunkType: chunk.chunkType,
      });
    }

    return {
      queryFunctions,
      warnings,
      meta: {
        model: embedder.modelName,
        indexedFunctions: indexedCount,
        queryFunctions: parsed.chunks.length,
        elapsedMs: Date.now() - start,
      },
    };
  } finally {
    db.close();
  }
}
