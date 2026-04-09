pub mod anthropic;
pub mod cost_db;
pub mod openai_compat;
pub mod pricing;

use anyhow::Result;

pub trait ProviderAdapter: Send + Sync {
    fn transform_request(&self, body: &serde_json::Value) -> Result<serde_json::Value>;
    fn transform_response(&self, body: &serde_json::Value) -> Result<serde_json::Value>;
    fn transform_stream_chunk(&self, chunk: &str) -> Result<String>;
    fn extract_usage(&self, body: &serde_json::Value) -> Option<(u64, u64)>;
    fn auth_header(&self, api_key: &str) -> String;
}
