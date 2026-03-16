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
        lines.push(`  ${m.a.name} (${relA}:${m.a.line})`);
        lines.push(`  ${m.b.name} (${relB}:${m.b.line})`);
        lines.push(`  ${pct}% similar (distance: ${m.distance.toFixed(4)})`);
        lines.push('');
      }
    }
  }

  lines.push(
    `${result.meta.pairsFound} pairs from ${result.meta.functionsScanned} functions (${result.meta.elapsedMs}ms)`,
  );

  return lines.join('\n');
}
