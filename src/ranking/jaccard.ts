/** Stop words to filter from tokenized code. */
const STOP_WORDS = new Set([
  'const', 'let', 'var', 'function', 'return', 'if', 'else', 'for', 'while',
  'do', 'switch', 'case', 'break', 'continue', 'try', 'catch', 'finally',
  'throw', 'new', 'this', 'typeof', 'instanceof', 'void', 'delete', 'in',
  'of', 'import', 'export', 'from', 'default', 'async', 'await', 'class',
  'extends', 'implements', 'interface', 'type', 'enum', 'true', 'false',
  'null', 'undefined',
]);

/** Strip type annotations (rough regex) and extract identifiers + operators. */
export function tokenizeCode(code: string): Set<string> {
  // Remove type annotations: `: Type`, `as Type`, generic brackets
  const stripped = code
    .replace(/:\s*[A-Z][\w<>,\s|&\[\]]*(?=[;,)=\n{])/g, '')
    .replace(/\bas\s+\w+/g, '')
    .replace(/<[A-Z][\w<>,\s|&]*>/g, '');

  // Extract identifiers and operators
  const tokens = stripped.match(/[a-zA-Z_$][\w$]*|[+\-*/%=<>!&|^~?:]+/g) ?? [];

  const result = new Set<string>();
  for (const token of tokens) {
    const lower = token.toLowerCase();
    if (!STOP_WORDS.has(lower) && lower.length > 1) {
      result.add(lower);
    }
  }
  return result;
}

export function jaccardSimilarity(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 && b.size === 0) return 1;

  let intersection = 0;
  const smaller = a.size <= b.size ? a : b;
  const larger = a.size <= b.size ? b : a;

  for (const token of smaller) {
    if (larger.has(token)) intersection++;
  }

  const union = a.size + b.size - intersection;
  return union === 0 ? 0 : intersection / union;
}

export interface RankedCandidate {
  name: string;
  path: string;
  line: number;
  distance: number;
  detectionMethod: 'embedding' | 'jscpd' | 'combined';
  source: string;
  signatureHash: string;
  chunkType?: 'function' | 'block';
  context?: string | null;
  jaccardSimilarity: number;
  combinedScore: number;
}

/**
 * Re-rank KNN candidates using combined embedding distance + Jaccard similarity.
 * Lower combinedScore = more similar.
 */
export function rerankCandidates(
  querySource: string,
  candidates: Array<{
    name: string;
    path: string;
    line: number;
    distance: number;
    detectionMethod: 'embedding' | 'jscpd' | 'combined';
    source: string;
    signatureHash: string;
    chunkType?: 'function' | 'block';
    context?: string | null;
  }>,
  alpha: number = 0.7,
): RankedCandidate[] {
  const queryTokens = tokenizeCode(querySource);

  const ranked = candidates.map((c) => {
    const candidateTokens = tokenizeCode(c.source);
    const jaccard = jaccardSimilarity(queryTokens, candidateTokens);
    // Combined: alpha * embedding_distance + (1-alpha) * (1 - jaccard)
    const combinedScore = alpha * c.distance + (1 - alpha) * (1 - jaccard);
    return { ...c, jaccardSimilarity: jaccard, combinedScore };
  });

  ranked.sort((a, b) => a.combinedScore - b.combinedScore);
  return ranked;
}
