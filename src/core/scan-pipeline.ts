import { openDatabase, getMetaValue } from '../store/db.js';
import { getAllFunctions, queryKNN } from '../store/index-store.js';
import { loadIgnoreFile, isExcluded } from '../ignore/ignore-file.js';
import { findProjectRoot, resolveDbPath } from '../util/config.js';
import type { ScanMatch, ScanResult } from '../output/scan-types.js';

export interface ScanOptions {
  topN?: number;
  threshold?: number;
  dbPath?: string;
  projectRoot?: string;
  onProgress?: (message: string) => void;
}

function similarityTier(distance: number): ScanMatch['similarity'] {
  if (distance <= 0.01) return 'identical';
  if (distance <= 0.05) return 'nearly identical';
  if (distance <= 0.12) return 'very similar';
  if (distance <= 0.20) return 'similar';
  return 'weak';
}

export function runScan(options: ScanOptions): ScanResult {
  const start = Date.now();
  const threshold = options.threshold ?? 0.25;
  const topN = options.topN ?? 50;
  const projectRoot = options.projectRoot ?? findProjectRoot(process.cwd());
  const dbPath = options.dbPath ?? resolveDbPath(projectRoot);
  const log = options.onProgress ?? (() => {});

  const db = openDatabase(dbPath);
  try {
    const functions = getAllFunctions(db);
    const model = getMetaValue(db, 'model_name') ?? 'unknown';
    const ignoreFile = loadIgnoreFile(projectRoot);

    const embedded = functions.filter((f) => f.embedding !== null);
    if (embedded.length === 0) {
      return {
        matches: [],
        meta: { model, functionsScanned: 0, pairsFound: 0, elapsedMs: Date.now() - start },
      };
    }

    log(`Scanning ${embedded.length} embedded functions...`);

    // For each function, find its nearest neighbors in the index.
    // We collect all (a, b, distance) pairs and deduplicate so each
    // unordered pair appears only once.
    const seen = new Set<string>();
    const allMatches: ScanMatch[] = [];

    for (let i = 0; i < embedded.length; i++) {
      const fn = embedded[i];
      const embedding = new Float32Array(
        fn.embedding!.buffer,
        fn.embedding!.byteOffset,
        fn.embedding!.byteLength / 4,
      );

      // +1 because the function will match itself at distance 0
      const neighbors = queryKNN(db, embedding, 6, threshold);

      for (const neighbor of neighbors) {
        // Skip self-match
        if (neighbor.id === fn.id) continue;

        // Deduplicate unordered pair
        const pairKey = fn.id < neighbor.id
          ? `${fn.id}||${neighbor.id}`
          : `${neighbor.id}||${fn.id}`;
        if (seen.has(pairKey)) continue;
        seen.add(pairKey);

        // Skip excluded pairs
        if (isExcluded(ignoreFile, fn.signatureHash, neighbor.signatureHash)) continue;

        // Order alphabetically by id for stable output
        const [a, b] = fn.id < neighbor.id
          ? [fn, neighbor]
          : [neighbor, fn];

        allMatches.push({
          a: { name: a.functionName, path: a.filePath, line: a.startLine, signatureHash: a.signatureHash },
          b: { name: b.functionName, path: b.filePath, line: b.startLine, signatureHash: b.signatureHash },
          distance: neighbor.distance,
          similarity: similarityTier(neighbor.distance),
        });
      }

      if ((i + 1) % 20 === 0) {
        log(`  ${i + 1}/${embedded.length} functions scanned`);
      }
    }

    // Sort by distance (most similar first), then cap at topN
    allMatches.sort((a, b) => a.distance - b.distance);
    const matches = allMatches.slice(0, topN);

    return {
      matches,
      meta: {
        model,
        functionsScanned: embedded.length,
        pairsFound: matches.length,
        elapsedMs: Date.now() - start,
      },
    };
  } finally {
    db.close();
  }
}
