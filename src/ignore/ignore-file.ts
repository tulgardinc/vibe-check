import fs from 'node:fs';
import path from 'node:path';
import { warn } from '../util/logger.js';
import type { Exclusion, IgnoreFile } from './types.js';

const FILENAME = '.codereuse-ignore.json';

export function loadIgnoreFile(projectRoot: string): IgnoreFile {
  const filePath = path.join(projectRoot, FILENAME);
  if (!fs.existsSync(filePath)) {
    return { version: 1, exclusions: [] };
  }
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    const parsed = JSON.parse(content) as Record<string, unknown>;
    if (parsed.version !== 1 || !Array.isArray(parsed.exclusions)) {
      warn(`${FILENAME} has unexpected format — treating as empty`);
      return { version: 1, exclusions: [] };
    }
    return parsed as unknown as IgnoreFile;
  } catch (e) {
    warn(`Failed to parse ${FILENAME}: ${e instanceof Error ? e.message : String(e)} — treating as empty`);
    return { version: 1, exclusions: [] };
  }
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
      (pairSideMatches(e.pair.a, exclusion.pair.a) &&
        pairSideMatches(e.pair.b, exclusion.pair.b)) ||
      (pairSideMatches(e.pair.a, exclusion.pair.b) &&
        pairSideMatches(e.pair.b, exclusion.pair.a)),
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

function pairSideMatches(
  a: { signatureHash: string },
  b: { signatureHash: string },
): boolean {
  return a.signatureHash === b.signatureHash;
}
