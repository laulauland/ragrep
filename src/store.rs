use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::path::Path;

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let db_url = format!("sqlite:{}", db_path.display());
        let pool = SqlitePool::connect(&db_url).await?;
        
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                text TEXT NOT NULL,
                embedding TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub async fn save_embedding(
        &self,
        file_path: &str,
        chunk_index: i32,
        text: &str,
        embedding: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO embeddings (file_path, chunk_index, text, embedding)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(file_path)
        .bind(chunk_index)
        .bind(text)
        .bind(embedding)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
