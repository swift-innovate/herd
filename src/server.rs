use crate::backend::{BackendPool, HealthChecker, ModelDiscovery};
use crate::config::Config;
use crate::router::{create_router, Router};
use crate::model_homing::ModelHoming;
use anyhow::Result;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

pub struct Server {
    config: Config,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<BackendPool>,
    pub router: crate::router::RouterEnum,
    pub client: Arc<reqwest::Client>,
    pub config: Config,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);
        info!("Starting Herd on {} with {} backends", addr, self.config.backends.len());

        // Create backend pool
        let pool = BackendPool::new(self.config.backends.clone());

        // Start health checker
        let health_checker = HealthChecker::new(10);
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            health_checker.spawn(pool_clone).await;
        });

        // Start model discovery (every 60 seconds)
        let discovery = ModelDiscovery::new(60);
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            discovery.spawn(pool_clone).await;
        });

        // Start model homing (every 5 minutes)
        let homing = ModelHoming::new(self.config.routing.idle_timeout_minutes);
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            homing.spawn(pool_clone).await;
        });

        // Create router
        let router = create_router(self.config.routing.strategy.clone(), pool.clone());

        // Wrap in Arc
        let pool = Arc::new(pool);
        let client = Arc::new(reqwest::Client::new());

        let state = AppState {
            pool: Arc::clone(&pool),
            router,
            client: Arc::clone(&client),
            config: self.config.clone(),
        };

        // Build app with routes
        let app = axum::Router::new()
            .route("/health", axum::routing::get(|| async { "OK" }))
            .route("/status", axum::routing::get(status_handler))
            .route("/metrics", axum::routing::get(metrics_handler))
            .route("/dashboard", axum::routing::get(dashboard_handler))
            .fallback(proxy_handler)
            .layer(tower::ServiceBuilder::new().layer(TraceLayer::new_for_http()))
            .with_state(state);

        // Start server
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
) -> Result<String, axum::http::StatusCode> {
    // Route request
    let backend = state
        .router
        .route(None)
        .await
        .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;

    // Touch the backend to update last_request time
    state.pool.touch_request(&backend.name).await;

    let url = format!("{}{}", backend.url, request.uri().path());
    
    // Get method from original request
    let method = match *request.method() {
        axum::http::Method::GET => reqwest::Method::GET,
        axum::http::Method::POST => reqwest::Method::POST,
        axum::http::Method::PUT => reqwest::Method::PUT,
        axum::http::Method::DELETE => reqwest::Method::DELETE,
        _ => reqwest::Method::POST,
    };
    
    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    let response = state
        .client
        .request(method, &url)
        .body(body_bytes)
        .send()
        .await
        .map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;

    response.text().await.map_err(|_| axum::http::StatusCode::BAD_GATEWAY)
}

async fn status_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::Json<serde_json::Value> {
    let all = state.pool.all().await;
    let mut healthy = Vec::new();
    let mut unhealthy = Vec::new();

    for name in all {
        if let Some(backend) = state.pool.get(&name).await {
            let mut backend_json = serde_json::json!({
                "name": backend.config.name,
                "url": backend.config.url,
                "priority": backend.config.priority,
                "models": backend.models,
                "current_model": backend.current_model,
                "default_model": backend.config.default_model,
            });

            if let Some(gpu) = &backend.gpu_metrics {
                backend_json["gpu"] = serde_json::json!({
                    "utilization": gpu.utilization,
                    "memory_used": gpu.memory_used,
                    "memory_total": gpu.memory_total,
                    "temperature": gpu.temperature,
                });
            }

            if backend.healthy {
                healthy.push(backend_json);
            } else {
                unhealthy.push(backend_json);
            }
        }
    }

    axum::Json(serde_json::json!({
        "healthy_backends": healthy,
        "unhealthy_backends": unhealthy,
        "routing_strategy": format!("{:?}", state.config.routing.strategy),
        "idle_timeout_minutes": state.config.routing.idle_timeout_minutes,
    }))
}

async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> String {
    let healthy = state.pool.all_healthy().await.len();
    let total = state.pool.all().await.len();
    
    let mut metrics = format!(
        r#"# HELP herd_backends_total Total number of configured backends
# TYPE herd_backends_total gauge
herd_backends_total {}

# HELP herd_backends_healthy Number of healthy backends
# TYPE herd_backends_healthy gauge
herd_backends_healthy {}

# HELP herd_backend_info Backend information
# TYPE herd_backend_info gauge
"#,
        total, healthy
    );

    for name in state.pool.all().await {
        if let Some(backend) = state.pool.get(&name).await {
            let labels = format!(
                r#"name="{}",priority="{}",healthy="{}""#,
                backend.config.name,
                backend.config.priority,
                backend.healthy
            );
            metrics.push_str(&format!("herd_backend_info{{{}}} 1\n", labels));
            
            if let Some(gpu) = &backend.gpu_metrics {
                metrics.push_str(&format!(
                    r#"herd_backend_gpu_utilization{{name="{}"}} {}
herd_backend_gpu_memory_used{{name="{}"}} {}
herd_backend_gpu_memory_total{{name="{}"}} {}
herd_backend_gpu_temperature{{name="{}"}} {}
"#,
                    backend.config.name, gpu.utilization,
                    backend.config.name, gpu.memory_used,
                    backend.config.name, gpu.memory_total,
                    backend.config.name, gpu.temperature
                ));
            }
        }
    }

    metrics
}

async fn dashboard_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::response::Html<String> {
    let all = state.pool.all().await;
    let mut backends_html = String::new();

    for name in all {
        if let Some(backend) = state.pool.get(&name).await {
            let status_class = if backend.healthy { "online" } else { "offline" };
            let status_text = if backend.healthy { "Online" } else { "Offline" };
            
            let gpu_html = if let Some(gpu) = &backend.gpu_metrics {
                format!(
                    r#"<div class="gpu">
                        <span>GPU: {:.0}%</span>
                        <span>VRAM: {:.1}GB/{:.1}GB</span>
                        <span>Temp: {:.0}°C</span>
                    </div>"#,
                    gpu.utilization,
                    gpu.memory_used as f64 / 1024.0,
                    gpu.memory_total as f64 / 1024.0,
                    gpu.temperature
                )
            } else {
                String::new()
            };

            let current_model = backend.current_model.as_deref().unwrap_or("None");
            let default_model = backend.config.default_model.as_deref().unwrap_or("None");
            let model_warning = if backend.current_model != backend.config.default_model && backend.config.default_model.is_some() {
                r#"<div class="warning">⚠️ Model differs from default</div>"#
            } else {
                ""
            };

            backends_html.push_str(&format!(
                r#"<div class="backend {}">
                    <div class="header">
                        <span class="name">{}</span>
                        <span class="status {}">● {}</span>
                    </div>
                    <div class="info">
                        <span>Priority: {}</span>
                        <span>Models: {}</span>
                    </div>
                    {}
                    <div class="models">
                        <div>Current: <strong>{}</strong></div>
                        <div>Default: {}</div>
                    </div>
                    {}
                </div>"#,
                status_class,
                backend.config.name,
                status_class, status_text,
                backend.config.priority,
                backend.models.len(),
                gpu_html,
                current_model,
                default_model,
                model_warning
            ));
        }
    }

    axum::response::Html(format!(r#"<!DOCTYPE html>
<html>
<head>
    <title>Herd Dashboard</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 20px; background: #1a1a2e; color: #eee; }}
        h1 {{ margin: 0 0 20px 0; }}
        .backends {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 16px; }}
        .backend {{ background: #16213e; border-radius: 8px; padding: 16px; border: 1px solid #0f3460; }}
        .backend.offline {{ opacity: 0.6; border-color: #e94560; }}
        .header {{ display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }}
        .name {{ font-size: 18px; font-weight: bold; }}
        .status {{ font-size: 14px; }}
        .status.online {{ color: #4ade80; }}
        .status.offline {{ color: #e94560; }}
        .info {{ display: flex; gap: 16px; color: #888; font-size: 14px; margin-bottom: 8px; }}
        .gpu {{ display: flex; gap: 16px; color: #60a5fa; font-size: 13px; margin-bottom: 8px; }}
        .models {{ font-size: 14px; margin-top: 8px; }}
        .warning {{ color: #fbbf24; margin-top: 8px; font-size: 13px; }}
        strong {{ color: #60a5fa; }}
    </style>
</head>
<body>
    <h1>🦙 Herd Dashboard</h1>
    <div class="backends">{}</div>
</body>
</html>"#, backends_html))
}

pub async fn run(config: Config) -> Result<()> {
    let server = Server::new(config);
    server.run().await
}