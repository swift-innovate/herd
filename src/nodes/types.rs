use serde::{Deserialize, Serialize};

/// Registration payload from herd-tune scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegistration {
    pub hostname: String,
    pub ollama_url: String,
    /// Stable machine identifier (preferred over hostname for upsert).
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub gpu: Option<String>,
    #[serde(default)]
    pub vram_mb: u32,
    #[serde(default)]
    pub ram_mb: u32,
    #[serde(default)]
    pub ollama_version: Option<String>,
    #[serde(default)]
    pub models_available: u32,
    #[serde(default)]
    pub models_loaded: Vec<String>,
    #[serde(default)]
    pub recommended_config: serde_json::Value,
    #[serde(default)]
    pub config_applied: bool,
    #[serde(default)]
    pub herd_tune_version: Option<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub registered_at: Option<String>,
}

/// Stored node record from SQLite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_id: Option<String>,
    pub hostname: String,
    pub ollama_url: String,
    pub gpu: Option<String>,
    pub vram_mb: u32,
    pub ram_mb: u32,
    pub max_concurrent: u32,
    pub ollama_version: Option<String>,
    pub os: Option<String>,
    pub status: String,
    pub priority: u32,
    pub enabled: bool,
    pub tags: Vec<String>,
    pub models_available: u32,
    pub models_loaded: Vec<String>,
    pub recommended_config: serde_json::Value,
    pub config_applied: bool,
    pub last_health_check: Option<String>,
    pub registered_at: String,
    pub updated_at: String,
}

/// Update payload for PUT /api/nodes/:id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub priority: Option<u32>,
    pub tags: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

/// Response after registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRegistrationResponse {
    pub id: String,
    pub hostname: String,
    pub status: String,
    pub message: String,
}
