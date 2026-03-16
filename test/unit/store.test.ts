import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import Database from 'better-sqlite3';
import * as sqliteVec from 'sqlite-vec';
import {
  upsertFunctions,
  getAllFunctions,
  deleteFunctionsForFile,
  updateEmbedding,
  queryKNN,
  getFunctionsWithoutEmbeddings,
  countFunctions,
} from '../../src/store/index-store.js';
import { openDatabase } from '../../src/store/db.js';
import type { FunctionChunk } from '../../src/parser/types.js';

const sampleChunk: FunctionChunk = {
  id: 'test.ts:foo:1',
  filePath: 'test.ts',
  functionName: 'foo',
  sourceText: 'function foo(x: number): number { return x * 2; }',
  startLine: 1,
  endLine: 3,
  params: [{ name: 'x', type: 'number' }],
  returnType: 'number',
  isExported: false,
  signatureHash: 'aabb1122',
};

describe('index-store', () => {
  let db: Database.Database;

  beforeEach(() => {
    db = openDatabase(':memory:');
    // Insert tracked file first (foreign key)
    db.prepare(
      'INSERT INTO tracked_files (file_path, content_hash, mtime_ms, indexed_at) VALUES (?, ?, ?, ?)',
    ).run('test.ts', 'hash123', 1000, new Date().toISOString());
  });

  afterEach(() => {
    db.close();
  });

  it('upserts and retrieves functions', () => {
    upsertFunctions(db, [sampleChunk]);
    const all = getAllFunctions(db);
    expect(all).toHaveLength(1);
    expect(all[0].functionName).toBe('foo');
    expect(all[0].signatureHash).toBe('aabb1122');
  });

  it('deletes functions for a file', () => {
    upsertFunctions(db, [sampleChunk]);
    deleteFunctionsForFile(db, 'test.ts');
    expect(getAllFunctions(db)).toHaveLength(0);
  });

  it('identifies functions without embeddings', () => {
    upsertFunctions(db, [sampleChunk]);
    const unembedded = getFunctionsWithoutEmbeddings(db);
    expect(unembedded).toHaveLength(1);
  });

  it('counts functions', () => {
    expect(countFunctions(db)).toBe(0);
    upsertFunctions(db, [sampleChunk]);
    expect(countFunctions(db)).toBe(1);
  });

  it('updates embedding and queries KNN', () => {
    upsertFunctions(db, [sampleChunk]);

    const embedding = new Float32Array([0.1, 0.2, 0.3, 0.4]);
    updateEmbedding(db, sampleChunk.id, embedding);

    const unembedded = getFunctionsWithoutEmbeddings(db);
    expect(unembedded).toHaveLength(0);

    const query = new Float32Array([0.1, 0.2, 0.3, 0.4]);
    const results = queryKNN(db, query, 5, 0.5);
    expect(results).toHaveLength(1);
    expect(results[0].functionName).toBe('foo');
    expect(results[0].distance).toBeCloseTo(0, 1);
  });

  it('KNN filters by threshold', () => {
    upsertFunctions(db, [sampleChunk]);
    const embedding = new Float32Array([0.1, 0.2, 0.3, 0.4]);
    updateEmbedding(db, sampleChunk.id, embedding);

    // Very different query vector
    const query = new Float32Array([-0.9, -0.8, -0.7, -0.6]);
    const results = queryKNN(db, query, 5, 0.1); // Very tight threshold
    expect(results).toHaveLength(0);
  });
});
