import Database from 'better-sqlite3';
import * as sqliteVec from 'sqlite-vec';

export function openDatabase(dbPath: string): Database.Database {
  const db = new Database(dbPath);
  db.pragma('journal_mode = WAL');
  db.pragma('foreign_keys = ON');
  sqliteVec.load(db);
  ensureSchema(db);
  return db;
}

function ensureSchema(db: Database.Database): void {
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
}

export function getMetaValue(db: Database.Database, key: string): string | null {
  const row = db.prepare('SELECT value FROM index_meta WHERE key = ?').get(key) as
    | { value: string }
    | undefined;
  return row?.value ?? null;
}

export function setMetaValue(db: Database.Database, key: string, value: string): void {
  db.prepare('INSERT OR REPLACE INTO index_meta (key, value) VALUES (?, ?)').run(
    key,
    value,
  );
}
