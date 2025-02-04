use std::sync::Arc;
use anyhow::Result;
use crate::embedder::Embedder;

pub struct AppContext {
    pub embedder: Arc<Embedder>,
}

impl AppContext {
    pub fn new() -> Result<Self> {
        Ok(Self {
            embedder: Arc::new(Embedder::new()?),
        })
    }
}
