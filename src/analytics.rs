use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLog {
    pub timestamp: i64, // Unix timestamp in seconds
    pub model: Option<String>,
    pub backend: String,
    pub duration_ms: u64,
    pub status: String, // "success" | "error"
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

pub struct Analytics {
    log_path: PathBuf,
    file_lock: Arc<Mutex<()>>,
}

impl Analytics {
    pub fn new() -> Result<Self> {
        let log_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
            .join(".herd");

        std::fs::create_dir_all(&log_dir)?;
        let log_path = log_dir.join("requests.jsonl");

        // Touch the file to ensure it exists, then drop the handle
        let _file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        drop(_file);

        Ok(Self {
            log_path,
            file_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn log_request(&self, log: RequestLog) -> Result<()> {
        let _guard = self.file_lock.lock().await;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        let json = serde_json::to_string(&log)?;
        writeln!(file, "{}", json)?;
        file.flush()?;
        Ok(())
    }

    pub async fn get_stats(&self, since_seconds: i64) -> Result<AnalyticsStats> {
        let _guard = self.file_lock.lock().await;
        let cutoff = chrono::Utc::now().timestamp() - since_seconds;

        let file = std::fs::File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        
        let mut total_requests = 0;
        let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let mut backend_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let _timeline: Vec<(i64, u64)> = Vec::new(); // (timestamp_minute, count)
        let mut minute_buckets: std::collections::HashMap<i64, u64> = std::collections::HashMap::new();
        let mut durations: Vec<u64> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if let Ok(log) = serde_json::from_str::<RequestLog>(&line) {
                if log.timestamp >= cutoff {
                    total_requests += 1;
                    
                    // Count by model
                    if let Some(model) = &log.model {
                        *model_counts.entry(model.clone()).or_insert(0) += 1;
                    }
                    
                    // Count by backend
                    *backend_counts.entry(log.backend.clone()).or_insert(0) += 1;
                    
                    // Timeline (group by minute)
                    let minute = (log.timestamp / 60) * 60;
                    *minute_buckets.entry(minute).or_insert(0) += 1;
                    
                    // Durations for percentiles
                    if log.status == "success" {
                        durations.push(log.duration_ms);
                    }
                }
            }
        }

        // Convert minute buckets to sorted timeline
        let mut timeline_vec: Vec<(i64, u64)> = minute_buckets.into_iter().collect();
        timeline_vec.sort_by_key(|(ts, _)| *ts);
        
        // Calculate percentiles
        durations.sort();
        let p50 = if durations.is_empty() { 0 } else { durations[durations.len() / 2] };
        let p95 = if durations.is_empty() { 0 } else { durations[(durations.len() * 95) / 100] };
        let p99 = if durations.is_empty() { 0 } else { durations[(durations.len() * 99) / 100] };

        Ok(AnalyticsStats {
            total_requests,
            model_counts,
            backend_counts,
            timeline: timeline_vec,
            latency_p50: p50,
            latency_p95: p95,
            latency_p99: p99,
        })
    }

    /// Rotates the log file if it exceeds max_size_mb.
    /// Keeps up to max_files rotated files (.1, .2, etc.)
    pub async fn rotate_if_needed(&self, max_size_mb: u64, max_files: u32) -> Result<bool> {
        if max_size_mb == 0 {
            return Ok(false); // rotation disabled
        }

        let _guard = self.file_lock.lock().await;

        let metadata = match std::fs::metadata(&self.log_path) {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        let size_mb = metadata.len() / (1024 * 1024);
        if size_mb < max_size_mb {
            return Ok(false); // not yet at limit
        }

        // Shift existing rotated files: .4 → .5 (deleted if > max_files), .3 → .4, .2 → .3, .1 → .2
        for i in (1..max_files).rev() {
            let from = self.log_path.with_extension(format!("jsonl.{}", i));
            let to = self.log_path.with_extension(format!("jsonl.{}", i + 1));
            if from.exists() {
                if i + 1 > max_files {
                    let _ = std::fs::remove_file(&from);
                } else {
                    let _ = std::fs::rename(&from, &to);
                }
            }
        }

        // Delete the oldest if it exceeds max_files
        let oldest = self.log_path.with_extension(format!("jsonl.{}", max_files + 1));
        if oldest.exists() {
            let _ = std::fs::remove_file(&oldest);
        }

        // Current → .1
        let rotated = self.log_path.with_extension("jsonl.1");
        std::fs::rename(&self.log_path, &rotated)?;

        // Create fresh empty log file
        let _file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        tracing::info!("Rotated log file (was {}MB)", size_mb);
        Ok(true)
    }

    pub async fn cleanup_old(&self, days: i64) -> Result<usize> {
        let _guard = self.file_lock.lock().await;
        let cutoff = chrono::Utc::now().timestamp() - (days * 86400);

        let file = std::fs::File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        
        let temp_path = self.log_path.with_extension("tmp");
        let mut temp_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;
        
        let mut _kept = 0;
        let mut removed = 0;

        for line in reader.lines() {
            let line = line?;
            if let Ok(log) = serde_json::from_str::<RequestLog>(&line) {
                if log.timestamp >= cutoff {
                    writeln!(temp_file, "{}", line)?;
                    _kept += 1;
                } else {
                    removed += 1;
                }
            }
        }

        temp_file.flush()?;
        std::fs::rename(temp_path, &self.log_path)?;

        Ok(removed)
    }
}

#[derive(Debug, Serialize)]
pub struct AnalyticsStats {
    pub total_requests: u64,
    pub model_counts: std::collections::HashMap<String, u64>,
    pub backend_counts: std::collections::HashMap<String, u64>,
    pub timeline: Vec<(i64, u64)>, // (timestamp, count)
    pub latency_p50: u64,
    pub latency_p95: u64,
    pub latency_p99: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_log_with_request_id_serializes() {
        let log = RequestLog {
            timestamp: 1000,
            model: Some("test".into()),
            backend: "b1".into(),
            duration_ms: 100,
            status: "success".into(),
            path: "/api/generate".into(),
            request_id: Some("abc-123".into()),
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("abc-123"));
    }

    #[test]
    fn request_log_without_request_id_omits_field() {
        let log = RequestLog {
            timestamp: 1000,
            model: None,
            backend: "b1".into(),
            duration_ms: 100,
            status: "success".into(),
            path: "/test".into(),
            request_id: None,
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(!json.contains("request_id"));
    }

    #[test]
    fn request_log_deserializes_without_request_id() {
        // Old logs without request_id field should still deserialize
        let json = r#"{"timestamp":1000,"model":null,"backend":"b1","duration_ms":100,"status":"success","path":"/test"}"#;
        let log: RequestLog = serde_json::from_str(json).unwrap();
        assert!(log.request_id.is_none());
    }

    #[test]
    fn config_defaults() {
        let config: crate::config::ObservabilityConfig = Default::default();
        assert_eq!(config.log_retention_days, 7);
        assert_eq!(config.log_max_size_mb, 100);
        assert_eq!(config.log_max_files, 5);
    }

    #[test]
    fn config_deserializes_log_settings() {
        let yaml = r#"
            metrics: true
            log_retention_days: 14
            log_max_size_mb: 50
            log_max_files: 3
        "#;
        let config: crate::config::ObservabilityConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.log_retention_days, 14);
        assert_eq!(config.log_max_size_mb, 50);
        assert_eq!(config.log_max_files, 3);
    }
}
