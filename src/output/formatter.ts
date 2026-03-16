import type { QueryResult } from './types.js';

export function formatJson(result: QueryResult): string {
  return JSON.stringify(result, null, 2);
}

export function formatHuman(result: QueryResult): string {
  const lines: string[] = [];

  for (const qf of result.query_functions) {
    lines.push(`\n${qf.name} (${qf.file}:${qf.line})`);

    if (qf.candidates.length === 0) {
      lines.push('  No similar functions found.');
      continue;
    }

    for (const c of qf.candidates) {
      const similarity = (1 - c.similarity).toFixed(2);
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
    `\n${result.meta.query_functions} functions checked against ${result.meta.indexed_functions} indexed (${result.meta.elapsed_ms}ms)`,
  );

  return lines.join('\n');
}
