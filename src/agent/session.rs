use crate::agent::types::{AgentMessage, SessionStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub model: String,
    pub messages: Vec<AgentMessage>,
    pub status: SessionStatus,
    pub created_at: i64,
    pub updated_at: i64,
}
