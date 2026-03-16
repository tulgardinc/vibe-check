import type Database from 'better-sqlite3';
import { contentHash } from '../util/hash.js';
import type { FunctionChunk } from '../parser/types.js';
import type { StoredFunction } from './types.js';

/** Expected shape of a row from the functions table. */
interface FunctionRow {
  id: string;
  file_path: string;
  function_name: string;
  source_text: string;
  start_line: number;
  end_line: number;
  params_json: string;
  return_type: string | null;
  is_exported: number;
  signature_hash: string;
  content_hash: string;
  embedding: Buffer | null;
}

export function upsertFunctions(
  db: Database.Database,
  chunks: FunctionChunk[],
): void {
  const stmt = db.prepare(`
    INSERT OR REPLACE INTO functions
      (id, file_path, function_name, source_text, start_line, end_line,
       params_json, return_type, is_exported, signature_hash, content_hash, embedding)
    VALUES
      (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
  `);

  const tx = db.transaction((items: FunctionChunk[]) => {
    for (const chunk of items) {
      stmt.run(
        chunk.id,
        chunk.filePath,
        chunk.functionName,
        chunk.sourceText,
        chunk.startLine,
        chunk.endLine,
        JSON.stringify(chunk.params),
        chunk.returnType,
        chunk.isExported ? 1 : 0,
        chunk.signatureHash,
        contentHash(chunk.sourceText),
      );
    }
  });
  tx(chunks);
}

export function deleteFunctionsForFile(
  db: Database.Database,
  filePath: string,
): void {
  db.prepare('DELETE FROM functions WHERE file_path = ?').run(filePath);
}

export function getFunctionBySignatureHash(
  db: Database.Database,
  hash: string,
): StoredFunction | null {
  const row = db
    .prepare('SELECT * FROM functions WHERE signature_hash = ? LIMIT 1')
    .get(hash) as FunctionRow | undefined;
  return row ? mapRow(row) : null;
}

export function getAllFunctions(db: Database.Database): StoredFunction[] {
  const rows = db.prepare('SELECT * FROM functions').all() as FunctionRow[];
  return rows.map(mapRow);
}

export function countFunctions(db: Database.Database): number {
  const row = db.prepare('SELECT COUNT(*) AS count FROM functions').get() as { count: number };
  return row.count;
}

export function getFunctionsWithoutEmbeddings(
  db: Database.Database,
): StoredFunction[] {
  const rows = db.prepare('SELECT * FROM functions WHERE embedding IS NULL').all() as FunctionRow[];
  return rows.map(mapRow);
}

export function updateEmbedding(
  db: Database.Database,
  functionId: string,
  embedding: Float32Array,
): void {
  db.prepare('UPDATE functions SET embedding = ? WHERE id = ?').run(
    Buffer.from(embedding.buffer),
    functionId,
  );
}

export function queryKNN(
  db: Database.Database,
  queryEmbedding: Float32Array,
  topK: number,
  threshold: number,
): (StoredFunction & { distance: number })[] {
  const queryBuf = Buffer.from(queryEmbedding.buffer);
  const rows = db
    .prepare(
      `SELECT *, vec_distance_cosine(embedding, ?) AS distance
       FROM functions
       WHERE embedding IS NOT NULL
       ORDER BY distance ASC
       LIMIT ?`,
    )
    .all(queryBuf, topK) as (FunctionRow & { distance: number })[];

  return rows
    .filter((r) => r.distance <= threshold)
    .map((r) => ({ ...mapRow(r), distance: r.distance }));
}

function mapRow(row: FunctionRow): StoredFunction {
  return {
    id: row.id,
    filePath: row.file_path,
    functionName: row.function_name,
    sourceText: row.source_text,
    startLine: row.start_line,
    endLine: row.end_line,
    paramsJson: row.params_json,
    returnType: row.return_type,
    isExported: row.is_exported === 1,
    signatureHash: row.signature_hash,
    contentHash: row.content_hash,
    embedding: row.embedding,
  };
}
