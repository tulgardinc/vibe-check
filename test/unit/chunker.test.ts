import { describe, it, expect } from 'vitest';
import { parseSource } from '../../src/parser/chunker.js';
import fs from 'node:fs';
import path from 'node:path';

const fixturePath = path.join(import.meta.dirname, '..', 'fixtures', 'sample.ts');
const fixtureSource = fs.readFileSync(fixturePath, 'utf-8');

describe('chunker', () => {
  it('extracts function declarations', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'calculateTotal');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(2);
    expect(fn!.params[0].name).toBe('items');
    expect(fn!.params[0].type).toBe('number[]');
    expect(fn!.returnType).toBe('number');
    expect(fn!.isExported).toBe(true);
  });

  it('extracts arrow functions assigned to const', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'formatCurrency');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(2);
    expect(fn!.params[1].name).toBe('currency');
    expect(fn!.params[1].type).toBe('string');
    expect(fn!.returnType).toBe('string');
  });

  it('extracts class methods', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'addUser');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(2);
    expect(fn!.returnType).toBe('void');
  });

  it('extracts generator functions', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'generateIds');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(1);
    expect(fn!.returnType).toBe('Generator<string>');
  });

  it('extracts function expressions', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'processData');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(1);
    expect(fn!.params[0].type).toBe('string[]');
  });

  it('skips tiny functions', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'tiny');
    expect(fn).toBeUndefined();
  });

  it('does not extract anonymous callbacks', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    // The .map callback should not appear as a chunk
    const names = result.chunks.map((c) => c.functionName);
    expect(names).not.toContain('');
    expect(names).not.toContain(undefined);
  });

  it('extracts class method getUser (3 lines, meets minimum)', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    const fn = result.chunks.find((c) => c.functionName === 'getUser');
    expect(fn).toBeDefined();
    expect(fn!.params).toHaveLength(1);
    expect(fn!.params[0].type).toBe('string');
  });

  it('generates deterministic IDs', () => {
    const result1 = parseSource(fixtureSource, 'sample.ts');
    const result2 = parseSource(fixtureSource, 'sample.ts');
    expect(result1.chunks.map((c) => c.id)).toEqual(
      result2.chunks.map((c) => c.id),
    );
  });

  it('reports no parse errors for valid TypeScript', () => {
    const result = parseSource(fixtureSource, 'sample.ts');
    expect(result.parseErrors).toHaveLength(0);
  });
});
