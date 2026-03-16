/** A deduplicated pair of similar functions found during a full codebase scan. */
export interface ScanMatch {
  /** Function A (alphabetically first by id). */
  a: {
    name: string;
    path: string;
    line: number;
    signatureHash: string;
  };
  /** Function B. */
  b: {
    name: string;
    path: string;
    line: number;
    signatureHash: string;
  };
  /** Cosine distance — lower means more similar (0 = identical). */
  distance: number;
  /** Human-readable similarity tier. */
  similarity: 'identical' | 'nearly identical' | 'very similar' | 'similar' | 'weak';
}

export interface ScanResult {
  matches: ScanMatch[];
  meta: {
    model: string;
    functionsScanned: number;
    pairsFound: number;
    elapsedMs: number;
  };
}
