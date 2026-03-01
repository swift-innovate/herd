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
            let idle_secs = backend.last_request.elapsed().as_secs();
            let mut backend_json = serde_json::json!({
                "name": backend.config.name,
                "url": backend.config.url,
                "priority": backend.config.priority,
                "models": backend.models,
                "model_count": backend.models.len(),
                "current_model": backend.current_model,
                "default_model": backend.config.default_model,
                "idle_seconds": idle_secs,
                "healthy": backend.healthy,
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

async fn dashboard_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../dashboard.html"))
}

pub async fn run(config: Config) -> Result<()> {
    let server = Server::new(config);
    server.run().await
}