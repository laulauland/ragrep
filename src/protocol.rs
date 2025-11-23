use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    pub top_n: usize,
    pub files_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub file_path: String,
    pub start_line: i32,
    pub end_line: i32,
    pub text: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub stats: SearchStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchStats {
    pub total_time_ms: u64,
    pub num_candidates: usize,
    pub num_results: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Message {
    Request { id: u64, request: SearchRequest },
    Response { id: u64, response: SearchResponse },
    Error { id: u64, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let request = Message::Request {
            id: 1,
            request: SearchRequest {
                query: "test".to_string(),
                top_n: 10,
                files_only: false,
            },
        };
        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: Message = serde_json::from_str(&serialized).unwrap();
        assert_eq!(request, deserialized);
    }
}
