use anyhow::Result;

pub struct Cleaner {
    chunk_size: usize,
}

impl Cleaner {
    pub fn new(chunk_size: usize) -> Self {
        Self { chunk_size }
    }

    pub fn clean_and_chunk(&self, content: &str) -> Result<Vec<String>> {
        let lines: Vec<&str> = content.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();

        let chunks = lines
            .chunks(self.chunk_size)
            .map(|chunk| chunk.join("\n"))
            .collect();

        Ok(chunks)
    }
}
