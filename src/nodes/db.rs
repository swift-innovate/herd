use super::types::*;
use anyhow::Result;
use rusqlite::Connection;
use std::sync::Mutex;

pub struct NodeDb {
    conn: Mutex<Connection>,
}

impl NodeDb {
    /// Open (or create) the database at ~/.herd/herd.db and run migrations.
    pub fn open() -> Result<Self> {
        let herd_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
            .join(".herd");
        std::fs::create_dir_all(&herd_dir)?;
        let db_path = herd_dir.join("herd.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                hostname TEXT NOT NULL UNIQUE,
                ollama_url TEXT NOT NULL,
                gpu TEXT,
                vram_mb INTEGER DEFAULT 0,
                ram_mb INTEGER DEFAULT 0,
                max_concurrent INTEGER DEFAULT 1,
                ollama_version TEXT,
                os TEXT,
                status TEXT DEFAULT 'healthy',
                priority INTEGER DEFAULT 10,
                enabled INTEGER DEFAULT 1,
                tags TEXT DEFAULT '[]',
                models_available INTEGER DEFAULT 0,
                models_loaded TEXT DEFAULT '[]',
                recommended_config TEXT DEFAULT '{}',
                config_applied INTEGER DEFAULT 0,
                last_health_check TEXT,
                registered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        // Migration v1: add stable machine identity column
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN node_id TEXT;")
            .ok(); // silently ignores "duplicate column" on subsequent runs
        // Create unique index separately (idempotent)
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_nodes_node_id ON nodes(node_id) WHERE node_id IS NOT NULL;",
        )?;

        // Migration v2: llama-server backend support
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN backend TEXT NOT NULL DEFAULT 'ollama';")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN backend_url TEXT NOT NULL DEFAULT '';")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN backend_version TEXT;")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN gpu_vendor TEXT;")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN gpu_model TEXT;")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN gpu_backend TEXT;")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN cuda_version TEXT;")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN model_paths TEXT DEFAULT '[]';")
            .ok();
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN capabilities TEXT DEFAULT '[]';")
            .ok();

        // Backfill: copy ollama_url -> backend_url for existing rows where backend_url is empty
        conn.execute_batch(
            "UPDATE nodes SET backend_url = ollama_url WHERE backend_url = '' AND ollama_url != '';",
        )?;

        Ok(())
    }

    /// Map a SELECT row (29 columns) to a Node struct.
    /// Column order matches NODE_COLUMNS:
    ///   id, node_id, hostname, backend_url, backend, backend_version,
    ///   gpu, gpu_vendor, gpu_model, gpu_backend, cuda_version,
    ///   vram_mb, ram_mb, max_concurrent,
    ///   ollama_version, os, status, priority, enabled, tags, models_available,
    ///   models_loaded, model_paths, capabilities,
    ///   recommended_config, config_applied, last_health_check,
    ///   registered_at, updated_at
    fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<Node> {
        let backend_str: String = row.get(4)?;
        let backend = match backend_str.as_str() {
            "llama-server" => crate::config::BackendType::LlamaServer,
            _ => crate::config::BackendType::Ollama,
        };
        Ok(Node {
            id: row.get(0)?,
            node_id: row.get(1)?,
            hostname: row.get(2)?,
            backend_url: row.get(3)?,
            backend,
            backend_version: row.get(5)?,
            gpu: row.get(6)?,
            gpu_vendor: row.get(7)?,
            gpu_model: row.get(8)?,
            gpu_backend: row.get(9)?,
            cuda_version: row.get(10)?,
            vram_mb: row.get::<_, i32>(11)? as u32,
            ram_mb: row.get::<_, i32>(12)? as u32,
            max_concurrent: row.get::<_, i32>(13)? as u32,
            ollama_version: row.get(14)?,
            os: row.get(15)?,
            status: row.get(16)?,
            priority: row.get::<_, i32>(17)? as u32,
            enabled: row.get::<_, i32>(18)? != 0,
            tags: serde_json::from_str(&row.get::<_, String>(19)?).unwrap_or_default(),
            models_available: row.get::<_, i32>(20)? as u32,
            models_loaded: serde_json::from_str(&row.get::<_, String>(21)?).unwrap_or_default(),
            model_paths: serde_json::from_str(&row.get::<_, String>(22)?).unwrap_or_default(),
            capabilities: serde_json::from_str(&row.get::<_, String>(23)?).unwrap_or_default(),
            recommended_config: serde_json::from_str(&row.get::<_, String>(24)?)
                .unwrap_or_default(),
            config_applied: row.get::<_, i32>(25)? != 0,
            last_health_check: row.get(26)?,
            registered_at: row.get(27)?,
            updated_at: row.get(28)?,
        })
    }

    const NODE_COLUMNS: &'static str =
        "id, node_id, hostname, backend_url, backend, backend_version,
         gpu, gpu_vendor, gpu_model, gpu_backend, cuda_version,
         vram_mb, ram_mb, max_concurrent,
         ollama_version, os, status, priority, enabled, tags, models_available,
         models_loaded, model_paths, capabilities,
         recommended_config, config_applied, last_health_check,
         registered_at, updated_at";

    /// Insert or update a node by hostname (idempotent registration).
    /// Returns (node_id, is_new).
    pub fn upsert_node(&self, reg: &NodeRegistration) -> Result<(String, bool)> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let now = chrono::Utc::now().to_rfc3339();
        let registered_at = reg.registered_at.clone().unwrap_or_else(|| now.clone());

        // Prefer node_id for identity (stable across hostname changes),
        // fall back to hostname for backward compatibility with old scripts.
        let existing_id: Option<String> = reg
            .node_id
            .as_ref()
            .and_then(|nid| {
                conn.query_row(
                    "SELECT id FROM nodes WHERE node_id = ?1",
                    rusqlite::params![nid],
                    |row| row.get(0),
                )
                .ok()
            })
            .or_else(|| {
                conn.query_row(
                    "SELECT id FROM nodes WHERE hostname = ?1",
                    rusqlite::params![reg.hostname],
                    |row| row.get(0),
                )
                .ok()
            });

        let models_loaded_json = serde_json::to_string(&reg.models_loaded)?;
        let model_paths_json = serde_json::to_string(&reg.model_paths)?;
        let capabilities_json = serde_json::to_string(&reg.capabilities)?;
        let recommended_config_json = serde_json::to_string(&reg.recommended_config)?;
        let backend_str = reg.backend.to_string();
        let backend_url = reg.effective_url().to_string();

        // Derive max_concurrent from recommended_config.num_parallel or default to 1
        let max_concurrent = reg
            .recommended_config
            .get("num_parallel")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        if let Some(id) = existing_id {
            // Update existing node (also upgrades node_id + hostname on re-registration)
            conn.execute(
                "UPDATE nodes SET
                    ollama_url = ?1, gpu = ?2, vram_mb = ?3, ram_mb = ?4,
                    max_concurrent = ?5, ollama_version = ?6, os = ?7,
                    status = 'healthy', models_available = ?8, models_loaded = ?9,
                    recommended_config = ?10, config_applied = ?11, updated_at = ?12,
                    node_id = COALESCE(?14, node_id), hostname = ?15,
                    backend = ?16, backend_url = ?17, backend_version = ?18,
                    gpu_vendor = ?19, gpu_model = ?20, gpu_backend = ?21,
                    cuda_version = ?22, model_paths = ?23, capabilities = ?24
                WHERE id = ?13",
                rusqlite::params![
                    backend_url,
                    reg.gpu,
                    reg.vram_mb,
                    reg.ram_mb,
                    max_concurrent,
                    reg.ollama_version,
                    reg.os,
                    reg.models_available,
                    models_loaded_json,
                    recommended_config_json,
                    reg.config_applied as i32,
                    now,
                    id,
                    reg.node_id,
                    reg.hostname,
                    backend_str,
                    backend_url,
                    reg.backend_version,
                    reg.gpu_vendor,
                    reg.gpu_model,
                    reg.gpu_backend,
                    reg.cuda_version,
                    model_paths_json,
                    capabilities_json
                ],
            )?;
            Ok((id, false))
        } else {
            // Insert new node
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO nodes (id, node_id, hostname, ollama_url, gpu, vram_mb, ram_mb,
                    max_concurrent, ollama_version, os, status, models_available,
                    models_loaded, recommended_config, config_applied,
                    registered_at, updated_at,
                    backend, backend_url, backend_version,
                    gpu_vendor, gpu_model, gpu_backend, cuda_version,
                    model_paths, capabilities)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'healthy', ?11, ?12, ?13, ?14, ?15, ?16,
                        ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                rusqlite::params![
                    id,
                    reg.node_id,
                    reg.hostname,
                    backend_url,
                    reg.gpu,
                    reg.vram_mb,
                    reg.ram_mb,
                    max_concurrent,
                    reg.ollama_version,
                    reg.os,
                    reg.models_available,
                    models_loaded_json,
                    recommended_config_json,
                    reg.config_applied as i32,
                    registered_at,
                    now,
                    backend_str,
                    backend_url,
                    reg.backend_version,
                    reg.gpu_vendor,
                    reg.gpu_model,
                    reg.gpu_backend,
                    reg.cuda_version,
                    model_paths_json,
                    capabilities_json
                ],
            )?;
            Ok((id, true))
        }
    }

    /// Get all nodes.
    pub fn list_nodes(&self) -> Result<Vec<Node>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let sql = format!(
            "SELECT {} FROM nodes ORDER BY priority ASC, hostname ASC",
            Self::NODE_COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let nodes = stmt
            .query_map([], Self::row_to_node)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nodes)
    }

    /// Get a single node by ID.
    pub fn get_node(&self, id: &str) -> Result<Option<Node>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let sql = format!(
            "SELECT {} FROM nodes WHERE id = ?1",
            Self::NODE_COLUMNS
        );
        let result = conn.query_row(&sql, rusqlite::params![id], Self::row_to_node);
        match result {
            Ok(node) => Ok(Some(node)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update operator-controlled fields (priority, tags, enabled).
    pub fn update_node(&self, id: &str, update: &NodeUpdate) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let now = chrono::Utc::now().to_rfc3339();

        // Check if node exists
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i32>(0),
        )? > 0;

        if !exists {
            return Ok(false);
        }

        if let Some(priority) = update.priority {
            conn.execute(
                "UPDATE nodes SET priority = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![priority as i32, &now, id],
            )?;
        }
        if let Some(ref tags) = update.tags {
            let tags_json = serde_json::to_string(tags)?;
            conn.execute(
                "UPDATE nodes SET tags = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![tags_json, &now, id],
            )?;
        }
        if let Some(enabled) = update.enabled {
            let status = if enabled { "healthy" } else { "disabled" };
            conn.execute(
                "UPDATE nodes SET enabled = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![enabled as i32, status, &now, id],
            )?;
        }

        Ok(true)
    }

    /// Delete a node.
    pub fn delete_node(&self, id: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let rows = conn.execute("DELETE FROM nodes WHERE id = ?1", rusqlite::params![id])?;
        Ok(rows > 0)
    }

    /// Update node health status and loaded models (called by health poller).
    pub fn update_health(
        &self,
        id: &str,
        status: &str,
        models_loaded: &[String],
        models_available: Option<u32>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let now = chrono::Utc::now().to_rfc3339();
        let models_json = serde_json::to_string(models_loaded)?;

        if let Some(avail) = models_available {
            conn.execute(
                "UPDATE nodes SET status = ?1, models_loaded = ?2, models_available = ?3, last_health_check = ?4, updated_at = ?4 WHERE id = ?5",
                rusqlite::params![status, models_json, avail as i32, now, id],
            )?;
        } else {
            conn.execute(
                "UPDATE nodes SET status = ?1, models_loaded = ?2, last_health_check = ?3, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![status, models_json, now, id],
            )?;
        }
        Ok(())
    }

    /// Get nodes that should be health-checked (enabled, not disabled by operator).
    pub fn get_pollable_nodes(&self) -> Result<Vec<Node>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let sql = format!(
            "SELECT {} FROM nodes WHERE enabled = 1",
            Self::NODE_COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let nodes = stmt
            .query_map([], Self::row_to_node)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nodes)
    }

    /// Get nodes eligible for routing (enabled + healthy/degraded).
    pub fn get_routable_nodes(&self) -> Result<Vec<Node>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
        let sql = format!(
            "SELECT {} FROM nodes WHERE enabled = 1 AND status IN ('healthy', 'degraded') ORDER BY priority ASC",
            Self::NODE_COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let nodes = stmt
            .query_map([], Self::row_to_node)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendType;

    fn test_db() -> NodeDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let db = NodeDb {
            conn: Mutex::new(conn),
        };
        db.migrate().unwrap();
        db
    }

    #[test]
    fn migrate_creates_table_and_columns() {
        let db = test_db();
        let reg = NodeRegistration {
            hostname: "test-node".to_string(),
            ollama_url: "http://test:11434".to_string(),
            backend: BackendType::Ollama,
            ..Default::default()
        };
        let (id, is_new) = db.upsert_node(&reg).unwrap();
        assert!(is_new);

        let node = db.get_node(&id).unwrap().unwrap();
        assert_eq!(node.backend, BackendType::Ollama);
        assert_eq!(node.backend_url, "http://test:11434");
    }

    #[test]
    fn upsert_llama_server_node() {
        let db = test_db();
        let reg = NodeRegistration {
            hostname: "citadel".to_string(),
            ollama_url: String::new(),
            backend_url: Some("http://citadel:8090".to_string()),
            backend: BackendType::LlamaServer,
            backend_version: Some("b8678".to_string()),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_model: Some("RTX 5090".to_string()),
            gpu_backend: Some("cuda".to_string()),
            cuda_version: Some("13.1".to_string()),
            vram_mb: 32768,
            models_loaded: vec!["gemma-4.gguf".to_string()],
            model_paths: vec!["/models/gemma-4.gguf".to_string()],
            capabilities: vec!["cuda".to_string(), "flash_attn".to_string()],
            ..Default::default()
        };
        let (id, is_new) = db.upsert_node(&reg).unwrap();
        assert!(is_new);

        let node = db.get_node(&id).unwrap().unwrap();
        assert_eq!(node.backend, BackendType::LlamaServer);
        assert_eq!(node.backend_url, "http://citadel:8090");
        assert_eq!(node.backend_version.as_deref(), Some("b8678"));
        assert_eq!(node.gpu_vendor.as_deref(), Some("nvidia"));
        assert_eq!(node.gpu_model.as_deref(), Some("RTX 5090"));
        assert_eq!(node.gpu_backend.as_deref(), Some("cuda"));
        assert_eq!(node.cuda_version.as_deref(), Some("13.1"));
        assert_eq!(node.vram_mb, 32768);
        assert_eq!(node.capabilities, vec!["cuda", "flash_attn"]);
        assert_eq!(node.model_paths, vec!["/models/gemma-4.gguf"]);
    }

    #[test]
    fn upsert_idempotent_re_registration() {
        let db = test_db();
        let reg = NodeRegistration {
            hostname: "node1".to_string(),
            ollama_url: "http://node1:11434".to_string(),
            backend: BackendType::Ollama,
            ..Default::default()
        };
        let (id1, new1) = db.upsert_node(&reg).unwrap();
        assert!(new1);

        let (id2, new2) = db.upsert_node(&reg).unwrap();
        assert!(!new2);
        assert_eq!(id1, id2);
    }

    #[test]
    fn routable_nodes_filters_correctly() {
        let db = test_db();
        let reg = NodeRegistration {
            hostname: "healthy-node".to_string(),
            ollama_url: "http://healthy:11434".to_string(),
            ..Default::default()
        };
        db.upsert_node(&reg).unwrap();

        let routable = db.get_routable_nodes().unwrap();
        assert_eq!(routable.len(), 1);
        assert_eq!(routable[0].hostname, "healthy-node");
    }

    #[test]
    fn update_preserves_backend_fields() {
        let db = test_db();
        let reg = NodeRegistration {
            hostname: "citadel".to_string(),
            backend_url: Some("http://citadel:8090".to_string()),
            backend: BackendType::LlamaServer,
            gpu_vendor: Some("nvidia".to_string()),
            ..Default::default()
        };
        let (id, _) = db.upsert_node(&reg).unwrap();

        // Update priority -- backend fields should be preserved
        db.update_node(
            &id,
            &NodeUpdate {
                priority: Some(200),
                tags: None,
                enabled: None,
            },
        )
        .unwrap();

        let node = db.get_node(&id).unwrap().unwrap();
        assert_eq!(node.priority, 200);
        assert_eq!(node.backend, BackendType::LlamaServer);
        assert_eq!(node.gpu_vendor.as_deref(), Some("nvidia"));
    }
}
