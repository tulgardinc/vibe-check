import type Database from 'better-sqlite3';
import { getFunctionBySignatureHash } from '../store/index-store.js';
import type { IgnoreFile, StaleWarning } from './types.js';

export function detectStaleExclusions(
  db: Database.Database,
  ignoreFile: IgnoreFile,
): StaleWarning[] {
  const warnings: StaleWarning[] = [];

  for (let i = 0; i < ignoreFile.exclusions.length; i++) {
    const exclusion = ignoreFile.exclusions[i];

    for (const side of ['a', 'b'] as const) {
      const ref = exclusion.pair[side];
      const stored = getFunctionBySignatureHash(db, ref.signatureHash);

      if (!stored) {
        warnings.push({
          exclusionIndex: i,
          side,
          functionName: ref.function,
          path: ref.path,
          reason: `Function "${ref.function}" in ${ref.path} no longer exists or its signature has changed`,
        });
      }
    }
  }

  return warnings;
}
