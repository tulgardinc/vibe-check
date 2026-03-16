import type { Candidate } from '../output/types.js';
import type { PrefilterMatch } from './types.js';

/**
 * Merge pre-filter matches with embedding-based candidates.
 * Pre-filter matches are boosted (they are high-confidence for Types 1-2).
 * Deduplicates by matched file path + line.
 */
export function mergeResults(
  prefilterMatches: PrefilterMatch[],
  embeddingCandidates: Candidate[],
): Candidate[] {
  const seen = new Map<string, Candidate>();

  // Pre-filter matches get priority
  for (const m of prefilterMatches) {
    const key = `${m.matchedFilePath}:${m.matchedStartLine}`;
    if (!seen.has(key)) {
      seen.set(key, {
        name: m.matchedFunction,
        path: m.matchedFilePath,
        line: m.matchedStartLine,
        distance: 1 - m.confidence, // Convert confidence to distance (lower = more similar)
        detectionMethod: 'jscpd',
        source: '',
        signatureHash: '',
      });
    }
  }

  // Add embedding candidates, upgrading to 'combined' if already seen from pre-filter
  for (const c of embeddingCandidates) {
    const key = `${c.path}:${c.line}`;
    const existing = seen.get(key);
    if (existing) {
      existing.detectionMethod = 'combined';
      // Keep the better (lower) distance score
      existing.distance = Math.min(existing.distance, c.distance);
      // Fill in source/signatureHash if the prefilter entry lacked them
      if (!existing.source && c.source) existing.source = c.source;
      if (!existing.signatureHash && c.signatureHash)
        existing.signatureHash = c.signatureHash;
    } else {
      seen.set(key, c);
    }
  }

  return Array.from(seen.values()).sort((a, b) => a.distance - b.distance);
}
