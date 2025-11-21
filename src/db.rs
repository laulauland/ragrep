use anyhow::Result;
use rusqlite::{params, Connection};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;
use zerocopy::IntoBytes;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &Path) -> Result<Self> {
        // Initialize sqlite-vec extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open(path)?;

        // Use query_row for PRAGMA that returns results.
        let _journal_mode: String =
            conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        // Create main table
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                node_type TEXT,
                node_name TEXT,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                text TEXT NOT NULL,
                hash INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(file_path, start_line, end_line, hash)
            );

            CREATE INDEX IF NOT EXISTS idx_file_path ON chunks(file_path);
            CREATE INDEX IF NOT EXISTS idx_chunk_index ON chunks(chunk_index);
            "#,
        )?;

        // Create vector table with dimensions (1024 is the dimension of our embeddings)
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(
            rowid INTEGER PRIMARY KEY,
            embedding FLOAT[1024]
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn save_chunk(
        &mut self,
        file_path: &str,
        chunk_index: i32,
        node_type: &str,
        node_name: Option<&str>,
        start_line: usize,
        end_line: usize,
        text: &str,
        chunk_hash: u64,
        embedding: &[f32],
    ) -> Result<()> {
        // Start a transaction to ensure both inserts succeed or fail together.
        let tx = self.conn.transaction()?;

        // Insert metadata into the chunks table.
        let rows = tx.execute(
            r#"
            INSERT OR IGNORE INTO chunks (
                file_path, chunk_index, node_type, node_name,
                start_line, end_line, text, hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            (
                file_path,
                chunk_index,
                node_type,
                node_name,
                start_line as i32,
                end_line as i32,
                text,
                chunk_hash as i64,
            ),
        )?;

        // Insert into chunks_vec only if a new row was added.
        if rows > 0 {
            let last_row_id = tx.last_insert_rowid();
            tx.execute(
                r#"
                INSERT OR IGNORE INTO chunks_vec (rowid, embedding) 
                VALUES (?1, ?2)
                "#,
                (last_row_id, embedding.as_bytes()),
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn find_similar_chunks(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, i32, i32, String, f32)>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT c.text, c.file_path, c.start_line, c.end_line, c.node_type, distance
            FROM chunks_vec
            JOIN chunks c ON c.id = chunks_vec.rowid
            WHERE embedding MATCH ?1 AND k = ?
            ORDER BY distance
            "#,
        )?;

        let chunks = stmt
            .query_map(params![query_embedding.as_bytes(), limit], |row| {
                Ok((
                    row.get(0)?, // text
                    row.get(1)?, // file_path
                    row.get(2)?, // start_line
                    row.get(3)?, // end_line
                    row.get(4)?, // node_type
                    row.get(5)?, // distance
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }
}
