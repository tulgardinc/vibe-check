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
} from '../../src/store/index-store.js';
import type { FunctionChunk } from '../../src/parser/types.js';

function createTestDb(): Database.Database {
  const db = new Database(':memory:');
  db.pragma('foreign_keys = ON');
  sqliteVec.load(db);

  db.exec(`
    CREATE TABLE IF NOT EXISTS index_meta (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS tracked_files (
      file_path TEXT PRIMARY KEY,
      content_hash TEXT NOT NULL,
      mtime_ms INTEGER NOT NULL,
      indexed_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS functions (
      id TEXT PRIMARY KEY,
      file_path TEXT NOT NULL,
      function_name TEXT NOT NULL,
      source_text TEXT NOT NULL,
      start_line INTEGER NOT NULL,
      end_line INTEGER NOT NULL,
      params_json TEXT NOT NULL,
      return_type TEXT,
      is_exported INTEGER NOT NULL,
      signature_hash TEXT NOT NULL,
      content_hash TEXT NOT NULL,
      embedding BLOB,
      FOREIGN KEY (file_path) REFERENCES tracked_files(file_path) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_functions_file ON functions(file_path);
    CREATE INDEX IF NOT EXISTS idx_functions_sig ON functions(signature_hash);
  `);

  return db;
}

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
    db = createTestDb();
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
