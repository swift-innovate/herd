use crate::backend::BackendPool;
use std::time::Duration;
use tokio::time::interval;

pub struct ModelWarmer {
    interval: Duration,
    client: reqwest::Client,
}

impl ModelWarmer {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval: Duration::from_secs(interval_secs),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    pub async fn spawn(self, pool: BackendPool) {
        tokio::spawn(async move {
            let mut ticker = interval(self.interval);
            loop {
                ticker.tick().await;
                self.warm_all(&pool).await;
            }
        });
    }

    async fn warm_all(&self, pool: &BackendPool) {
        let backends = pool.all().await;
        for name in backends {
            if let Some(state) = pool.get(&name).await {
                for model in &state.config.hot_models {
                    let url = warm_url(&state.config.url);
                    let payload = warm_payload(model);
                    let client = self.client.clone();
                    let model = model.clone();
                    let name = name.clone();
                    tokio::spawn(async move {
                        if let Err(e) = client.post(&url).json(&payload).send().await {
                            tracing::warn!("Warmer failed for {} on {}: {}", model, name, e);
                        } else {
                            tracing::debug!("Warmed {} on {}", model, name);
                        }
                    });
                }
            }
        }
    }
}

pub fn warm_url(base_url: &str) -> String {
    format!("{}/api/generate", base_url.trim_end_matches('/'))
}

pub fn warm_payload(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "prompt": "",
        "keep_alive": "-1"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_url_constructed_correctly() {
        let url = warm_url("http://citadel:11434");
        assert_eq!(url, "http://citadel:11434/api/generate");
    }

    #[test]
    fn warm_payload_contains_keep_alive() {
        let payload = warm_payload("llama3:8b");
        assert_eq!(payload["model"], "llama3:8b");
        assert_eq!(payload["keep_alive"], "-1");
        assert_eq!(payload["prompt"], "");
    }
}
