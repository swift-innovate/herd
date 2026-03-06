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
