interface ScanMatchEntry {
  name: string;
  path: string;
  line: number;
  signatureHash: string;
  chunkType?: 'function' | 'block';
  context?: string | null;
}

/** A deduplicated pair of similar functions/blocks found during a full codebase scan. */
export interface ScanMatch {
  /** Entry A (alphabetically first by id). */
  a: ScanMatchEntry;
  /** Entry B. */
  b: ScanMatchEntry;
  /** Combined score (embedding + Jaccard) — lower means more similar. */
  distance: number;
  /** Human-readable similarity tier. */
  similarity: 'identical' | 'nearly identical' | 'very similar' | 'similar' | 'weak';
  jaccardSimilarity?: number;
}

export interface ScanResult {
  matches: ScanMatch[];
  meta: {
    model: string;
    chunksScanned: number;
    pairsFound: number;
    elapsedMs: number;
  };
}
