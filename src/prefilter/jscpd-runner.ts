import { warn } from '../util/logger.js';
import type { PrefilterMatch } from './types.js';

export async function runJscpd(
  queryPaths: string[],
  indexedPaths: string[],
): Promise<PrefilterMatch[]> {
  try {
    const { detectClones } = await import('jscpd');

    const allPaths = [...queryPaths, ...indexedPaths];
    const clones = await detectClones({
      path: allPaths,
      silent: true,
      minLines: 5,
      minTokens: 50,
      format: ['typescript'],
    });

    const matches: PrefilterMatch[] = [];
    const querySet = new Set(queryPaths);

    for (const clone of clones) {
      const duplicationA = clone.duplicationA;
      const duplicationB = clone.duplicationB;
      const aIsQuery = querySet.has(duplicationA.sourceId);
      const bIsQuery = querySet.has(duplicationB.sourceId);

      // We only care about pairs where one side is query and the other is indexed
      if (aIsQuery === bIsQuery) continue;

      const [query, indexed] = aIsQuery
        ? [duplicationA, duplicationB]
        : [duplicationB, duplicationA];

      matches.push({
        queryFunction: query.sourceId,
        matchedFunction: indexed.sourceId,
        matchedFilePath: indexed.sourceId,
        matchedStartLine: indexed.start.line,
        cloneType: 1,
        tool: 'jscpd',
        confidence: 0.95,
      });
    }

    return matches;
  } catch (e) {
    warn(`jscpd pre-filter unavailable: ${e instanceof Error ? e.message : String(e)}`);
    return [];
  }
}
