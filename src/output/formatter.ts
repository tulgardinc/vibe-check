import type { QueryResult } from './types.js';

export function formatJson(result: QueryResult): string {
  return JSON.stringify(result, null, 2);
}

export function formatHuman(result: QueryResult): string {
  const lines: string[] = [];

  for (const qf of result.queryFunctions) {
    lines.push(`\n${qf.name} (${qf.file}:${qf.line})`);

    if (qf.candidates.length === 0) {
      lines.push('  No similar functions found.');
      continue;
    }

    for (const c of qf.candidates) {
      const similarity = (1 - c.distance).toFixed(2);
      lines.push(
        `  ${c.name} (${c.path}:${c.line}) — similarity: ${similarity} [${c.detectionMethod}]`,
      );
    }
  }

  if (result.warnings.length > 0) {
    lines.push('\nWarnings:');
    for (const w of result.warnings) {
      lines.push(`  ${w}`);
    }
  }

  lines.push(
    `\n${result.meta.queryFunctions} functions checked against ${result.meta.indexedFunctions} indexed (${result.meta.elapsedMs}ms)`,
  );

  return lines.join('\n');
}
