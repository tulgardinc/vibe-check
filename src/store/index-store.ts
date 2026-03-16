import type Database from 'better-sqlite3';
import { contentHash } from '../util/hash.js';
import type { FunctionChunk } from '../parser/types.js';
import type { StoredFunction } from './types.js';

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
    .get(hash);
  return row ? mapRow(row) : null;
}

export function getAllFunctions(db: Database.Database): StoredFunction[] {
  const rows = db.prepare('SELECT * FROM functions').all();
  return rows.map(mapRow);
}

export function getFunctionsWithoutEmbeddings(
  db: Database.Database,
): StoredFunction[] {
  const rows = db.prepare('SELECT * FROM functions WHERE embedding IS NULL').all();
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
    .all(queryBuf, topK) as (Record<string, unknown> & { distance: number })[];

  return rows
    .filter((r) => r.distance <= threshold)
    .map((r) => ({ ...mapRow(r), distance: r.distance }));
}

function mapRow(row: unknown): StoredFunction {
  const r = row as Record<string, unknown>;
  return {
    id: r.id as string,
    filePath: r.file_path as string,
    functionName: r.function_name as string,
    sourceText: r.source_text as string,
    startLine: r.start_line as number,
    endLine: r.end_line as number,
    paramsJson: r.params_json as string,
    returnType: r.return_type as string | null,
    isExported: (r.is_exported as number) === 1,
    signatureHash: r.signature_hash as string,
    contentHash: r.content_hash as string,
    embedding: r.embedding as Buffer | null,
  };
}
