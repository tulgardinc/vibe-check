import fs from 'node:fs';
import path from 'node:path';
import type { Exclusion, IgnoreFile } from './types.js';

const FILENAME = '.codereuse-ignore.json';

export function loadIgnoreFile(projectRoot: string): IgnoreFile {
  const filePath = path.join(projectRoot, FILENAME);
  if (!fs.existsSync(filePath)) {
    return { version: 1, exclusions: [] };
  }
  const content = fs.readFileSync(filePath, 'utf-8');
  return JSON.parse(content) as IgnoreFile;
}

export function saveIgnoreFile(projectRoot: string, ignoreFile: IgnoreFile): void {
  const filePath = path.join(projectRoot, FILENAME);
  fs.writeFileSync(filePath, JSON.stringify(ignoreFile, null, 2) + '\n', 'utf-8');
}

export function addExclusion(
  ignoreFile: IgnoreFile,
  exclusion: Exclusion,
): IgnoreFile {
  // Deduplicate: check if this pair already exists (in either direction)
  const exists = ignoreFile.exclusions.some(
    (e) =>
      (matchesSide(e.pair.a, exclusion.pair.a) &&
        matchesSide(e.pair.b, exclusion.pair.b)) ||
      (matchesSide(e.pair.a, exclusion.pair.b) &&
        matchesSide(e.pair.b, exclusion.pair.a)),
  );

  if (exists) return ignoreFile;

  return {
    ...ignoreFile,
    exclusions: [...ignoreFile.exclusions, exclusion],
  };
}

export function isExcluded(
  ignoreFile: IgnoreFile,
  queryHash: string,
  candidateHash: string,
): boolean {
  return ignoreFile.exclusions.some(
    (e) =>
      (e.pair.a.signatureHash === queryHash &&
        e.pair.b.signatureHash === candidateHash) ||
      (e.pair.a.signatureHash === candidateHash &&
        e.pair.b.signatureHash === queryHash),
  );
}

export function applyExclusions(
  ignoreFile: IgnoreFile,
  candidates: Array<{ signatureHash: string }>,
  querySignatureHash: string,
): Array<{ signatureHash: string }> {
  return candidates.filter(
    (c) => !isExcluded(ignoreFile, querySignatureHash, c.signatureHash),
  );
}

function matchesSide(
  a: { signatureHash: string },
  b: { signatureHash: string },
): boolean {
  return a.signatureHash === b.signatureHash;
}
