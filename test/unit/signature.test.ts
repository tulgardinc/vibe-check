import { describe, it, expect } from 'vitest';
import { computeSignatureHash } from '../../src/parser/signature.js';

describe('computeSignatureHash', () => {
  it('produces an 8-char hex string', () => {
    const hash = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    expect(hash).toMatch(/^[a-f0-9]{8}$/);
  });

  it('is deterministic', () => {
    const a = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    const b = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    expect(a).toBe(b);
  });

  it('is stable across whitespace in types', () => {
    const a = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    const b = computeSignatureHash('foo', [{ name: 'x', type: ' number ' }], ' string ');
    expect(a).toBe(b);
  });

  it('is case-insensitive for function name', () => {
    const a = computeSignatureHash('FooBar', [{ name: 'x', type: 'number' }], 'string');
    const b = computeSignatureHash('foobar', [{ name: 'x', type: 'number' }], 'string');
    expect(a).toBe(b);
  });

  it('differs when param types change', () => {
    const a = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    const b = computeSignatureHash('foo', [{ name: 'x', type: 'string' }], 'string');
    expect(a).not.toBe(b);
  });

  it('differs when return type changes', () => {
    const a = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'string');
    const b = computeSignatureHash('foo', [{ name: 'x', type: 'number' }], 'number');
    expect(a).not.toBe(b);
  });

  it('handles null types as any/void', () => {
    const hash = computeSignatureHash('foo', [{ name: 'x', type: null }], null);
    expect(hash).toMatch(/^[a-f0-9]{8}$/);
  });
});
