use anyhow::Result;
use sqlx::sqlite::SqlitePool;

pub struct QueryEngine {
    pool: SqlitePool,
}

impl QueryEngine {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<(String, f32)>> {
        // TODO: Implement similarity search
        Ok(vec![])
    }
}
