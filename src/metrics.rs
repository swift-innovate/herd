use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory Prometheus-compatible metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Total requests by status ("success" / "error")
    pub requests_total: Arc<RwLock<HashMap<String, AtomicU64>>>,
    /// Total requests by backend name
    pub requests_by_backend: Arc<RwLock<HashMap<String, AtomicU64>>>,
    /// Latency histogram buckets (in milliseconds)
    /// Buckets: 10, 50, 100, 250, 500, 1000, 2500, 5000, 10000, +Inf
    pub latency_buckets: Arc<RwLock<LatencyHistogram>>,
}

pub struct LatencyHistogram {
    bucket_bounds: Vec<u64>,    // upper bounds in ms
    bucket_counts: Vec<AtomicU64>, // count of observations <= bound
    sum: AtomicU64,             // total sum of observations in ms
    count: AtomicU64,           // total count of observations
}

impl LatencyHistogram {
    pub fn new() -> Self {
        let bounds = vec![10, 50, 100, 250, 500, 1000, 2500, 5000, 10000];
        let counts: Vec<AtomicU64> = bounds.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            bucket_bounds: bounds,
            bucket_counts: counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, value_ms: u64) {
        self.sum.fetch_add(value_ms, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        // Increment only the first matching bucket (render accumulates for Prometheus)
        for (i, bound) in self.bucket_bounds.iter().enumerate() {
            if value_ms <= *bound {
                self.bucket_counts[i].fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        // value exceeds all bucket bounds — counted in +Inf via self.count
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# HELP herd_request_duration_ms Request duration histogram in milliseconds\n");
        out.push_str("# TYPE herd_request_duration_ms histogram\n");
        // Buckets are cumulative in Prometheus
        let mut cumulative = 0u64;
        for (i, bound) in self.bucket_bounds.iter().enumerate() {
            cumulative += self.bucket_counts[i].load(Ordering::Relaxed);
            out.push_str(&format!(
                "herd_request_duration_ms_bucket{{le=\"{}\"}} {}\n",
                bound, cumulative
            ));
        }
        let total = self.count.load(Ordering::Relaxed);
        out.push_str(&format!(
            "herd_request_duration_ms_bucket{{le=\"+Inf\"}} {}\n",
            total
        ));
        out.push_str(&format!(
            "herd_request_duration_ms_sum {}\n",
            self.sum.load(Ordering::Relaxed)
        ));
        out.push_str(&format!("herd_request_duration_ms_count {}\n", total));
        out
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            requests_total: Arc::new(RwLock::new(HashMap::new())),
            requests_by_backend: Arc::new(RwLock::new(HashMap::new())),
            latency_buckets: Arc::new(RwLock::new(LatencyHistogram::new())),
        }
    }

    pub async fn record_request(&self, backend: &str, status: &str, duration_ms: u64) {
        // Increment by status
        {
            let mut map = self.requests_total.write().await;
            map.entry(status.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }
        // Increment by backend
        {
            let mut map = self.requests_by_backend.write().await;
            map.entry(backend.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }
        // Record latency
        {
            let hist = self.latency_buckets.read().await;
            hist.observe(duration_ms);
        }
    }

    pub async fn render(&self) -> String {
        let mut out = String::new();

        // Request totals by status
        out.push_str("# HELP herd_requests_total Total proxied requests by status\n");
        out.push_str("# TYPE herd_requests_total counter\n");
        {
            let map = self.requests_total.read().await;
            for (status, count) in map.iter() {
                out.push_str(&format!(
                    "herd_requests_total{{status=\"{}\"}} {}\n",
                    status,
                    count.load(Ordering::Relaxed)
                ));
            }
        }

        // Request totals by backend
        out.push_str("\n# HELP herd_requests_by_backend Total requests by backend\n");
        out.push_str("# TYPE herd_requests_by_backend counter\n");
        {
            let map = self.requests_by_backend.read().await;
            for (backend, count) in map.iter() {
                out.push_str(&format!(
                    "herd_requests_by_backend{{backend=\"{}\"}} {}\n",
                    backend,
                    count.load(Ordering::Relaxed)
                ));
            }
        }

        // Latency histogram
        out.push('\n');
        {
            let hist = self.latency_buckets.read().await;
            out.push_str(&hist.render());
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_and_renders_metrics() {
        let m = Metrics::new();
        m.record_request("backend-a", "success", 150).await;
        m.record_request("backend-a", "success", 50).await;
        m.record_request("backend-b", "error", 5000).await;

        let output = m.render().await;
        assert!(output.contains("herd_requests_total{status=\"success\"} 2"));
        assert!(output.contains("herd_requests_total{status=\"error\"} 1"));
        assert!(output.contains("herd_requests_by_backend{backend=\"backend-a\"} 2"));
        assert!(output.contains("herd_request_duration_ms_count 3"));
    }

    #[test]
    fn histogram_buckets_cumulative() {
        let h = LatencyHistogram::new();
        h.observe(5);   // fits in 10ms bucket
        h.observe(75);  // fits in 100ms bucket
        h.observe(300); // fits in 500ms bucket

        let rendered = h.render();
        // 10ms bucket should have 1 (the 5ms observation)
        assert!(rendered.contains("le=\"10\"} 1"));
        // 100ms bucket should have 2 cumulative (5ms + 75ms)
        assert!(rendered.contains("le=\"100\"} 2"));
        // 500ms bucket should have 3 cumulative
        assert!(rendered.contains("le=\"500\"} 3"));
    }
}
