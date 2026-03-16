import path from 'node:path';
import type { ScanResult, ScanMatch } from './scan-types.js';

export function formatScanJson(result: ScanResult): string {
  return JSON.stringify(result, null, 2);
}

export function formatScanHuman(result: ScanResult, projectRoot: string): string {
  const lines: string[] = [];

  if (result.matches.length === 0) {
    lines.push('No similar function pairs found.');
  } else {
    // Group by similarity tier for a cleaner report
    const tiers: Array<ScanMatch['similarity']> = ['identical', 'nearly identical', 'very similar', 'similar', 'weak'];
    for (const tier of tiers) {
      const group = result.matches.filter((m) => m.similarity === tier);
      if (group.length === 0) continue;

      lines.push(`\n── ${tier.toUpperCase()} (${group.length}) ${'─'.repeat(Math.max(0, 50 - tier.length))}`)

      for (const m of group) {
        const relA = path.relative(projectRoot, m.a.path);
        const relB = path.relative(projectRoot, m.b.path);
        const pct = ((1 - m.distance) * 100).toFixed(0);
        const nameA = m.a.chunkType === 'block' && m.a.context ? `${m.a.name} in ${m.a.context}` : m.a.name;
        const nameB = m.b.chunkType === 'block' && m.b.context ? `${m.b.name} in ${m.b.context}` : m.b.name;
        const jaccardInfo = m.jaccardSimilarity != null ? `, jaccard: ${m.jaccardSimilarity.toFixed(2)}` : '';
        lines.push(`  ${nameA} (${relA}:${m.a.line})`);
        lines.push(`  ${nameB} (${relB}:${m.b.line})`);
        lines.push(`  ${pct}% similar (distance: ${m.distance.toFixed(4)}${jaccardInfo})`);
        lines.push('');
      }
    }
  }

  lines.push(
    `${result.meta.pairsFound} pairs from ${result.meta.chunksScanned} chunks (${result.meta.elapsedMs}ms)`,
  );

  return lines.join('\n');
}
