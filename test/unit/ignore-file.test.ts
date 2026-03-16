import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {
  loadIgnoreFile,
  saveIgnoreFile,
  addExclusion,
  isExcluded,
  applyExclusions,
} from '../../src/ignore/ignore-file.js';
import type { Exclusion, IgnoreFile } from '../../src/ignore/types.js';

let tmpDir: string;

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'codeuse-test-'));
});

afterEach(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

const sampleExclusion: Exclusion = {
  reason: 'Different contracts',
  added: '2024-03-15',
  pair: {
    a: { path: 'src/a.ts', function: 'foo', signatureHash: 'aaaa1111' },
    b: { path: 'src/b.ts', function: 'bar', signatureHash: 'bbbb2222' },
  },
};

describe('ignore-file', () => {
  it('returns empty ignore file when none exists', () => {
    const result = loadIgnoreFile(tmpDir);
    expect(result.version).toBe(1);
    expect(result.exclusions).toHaveLength(0);
  });

  it('round-trips save and load', () => {
    const ignoreFile: IgnoreFile = {
      version: 1,
      exclusions: [sampleExclusion],
    };
    saveIgnoreFile(tmpDir, ignoreFile);
    const loaded = loadIgnoreFile(tmpDir);
    expect(loaded).toEqual(ignoreFile);
  });

  it('addExclusion appends to the list', () => {
    const empty: IgnoreFile = { version: 1, exclusions: [] };
    const updated = addExclusion(empty, sampleExclusion);
    expect(updated.exclusions).toHaveLength(1);
  });

  it('addExclusion deduplicates identical pairs', () => {
    const withOne = addExclusion({ version: 1, exclusions: [] }, sampleExclusion);
    const withDupe = addExclusion(withOne, sampleExclusion);
    expect(withDupe.exclusions).toHaveLength(1);
  });

  it('addExclusion deduplicates reversed pairs', () => {
    const reversed: Exclusion = {
      ...sampleExclusion,
      pair: { a: sampleExclusion.pair.b, b: sampleExclusion.pair.a },
    };
    const withOne = addExclusion({ version: 1, exclusions: [] }, sampleExclusion);
    const withReversed = addExclusion(withOne, reversed);
    expect(withReversed.exclusions).toHaveLength(1);
  });

  it('isExcluded returns true for known pair', () => {
    const ignoreFile: IgnoreFile = { version: 1, exclusions: [sampleExclusion] };
    expect(isExcluded(ignoreFile, 'aaaa1111', 'bbbb2222')).toBe(true);
  });

  it('isExcluded returns true for reversed pair', () => {
    const ignoreFile: IgnoreFile = { version: 1, exclusions: [sampleExclusion] };
    expect(isExcluded(ignoreFile, 'bbbb2222', 'aaaa1111')).toBe(true);
  });

  it('isExcluded returns false for unknown pair', () => {
    const ignoreFile: IgnoreFile = { version: 1, exclusions: [sampleExclusion] };
    expect(isExcluded(ignoreFile, 'aaaa1111', 'cccc3333')).toBe(false);
  });

  it('applyExclusions filters out excluded candidates', () => {
    const ignoreFile: IgnoreFile = { version: 1, exclusions: [sampleExclusion] };
    const candidates = [
      { signatureHash: 'bbbb2222', name: 'bar' },
      { signatureHash: 'cccc3333', name: 'baz' },
    ];
    const filtered = applyExclusions(ignoreFile, candidates, 'aaaa1111');
    expect(filtered).toHaveLength(1);
    expect((filtered[0] as { name: string }).name).toBe('baz');
  });
});
