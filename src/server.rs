use crate::api::{admin, metrics, status};
use crate::backend::{BackendPool, HealthChecker, ModelDiscovery};
use crate::config::Config;
use crate::router::{create_router, Router};
use anyhow::Result;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    routing::{get, post},
    Json,
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::info;

pub struct Server {
    config: Config,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);
        info!("Starting Herd on {}", addr);

        // Create backend pool
        let pool = BackendPool::new(self.config.backends.clone());

        // Start health checker
        let health_checker = HealthChecker::new(10);
        health_checker.spawn(pool.clone());

        // Start model discovery
        let discovery = ModelDiscovery::new(300);
        discovery.spawn(pool.clone());

        // Create router
        let router = create_router(self.config.routing.strategy.clone(), pool.clone());

        // Build app
        let app = axum::Router::new()
            .route("/status", get(status::get_status))
            .route("/metrics", get(metrics::get_metrics))
            .route("/health", get(health_check))
            .route("/admin/backends", post(admin::add_backend))
            .route("/admin/backends/remove", post(admin::remove_backend))
            .route("/admin/backends/drain", post(admin::drain_backend))
            .route("/*path", get(proxy_request).post(proxy_request).put(proxy_request).delete(proxy_request))
            .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
            .with_state(Arc::new(AppState {
                pool: pool.clone(),
                router,
            }));

        // Start server
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

struct AppState {
    pool: BackendPool,
    router: Box<dyn Router>,
}

async fn proxy_request(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<String, StatusCode> {
    // Get model from request body if present
    let model = extract_model(&request).await;
    
    // Route request
    let backend_url = state
        .router
        .route(model.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("Routing failed: {}", e);
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    // Forward request to backend
    let client = reqwest::Client::new();
    let url = format!("{}{}", backend_url, request.uri().path());
    
    let response = client
        .request(request.method().clone(), &url)
        .headers(request.headers().clone())
        .body(request.into_body())
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Backend request failed: {}", e);
            StatusCode::BAD_GATEWAY
        })?;

    let body = response.text().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(body)
}

async fn extract_model(request: &Request) -> Option<String> {
    // Try to parse model from request body
    // For generate requests: {"model": "llama3"}
    let body = request.body().as_ref()?;
    let parsed: serde_json::Value = serde_json::from_slice(body).ok()?;
    parsed.get("model")?.as_str().map(|s| s.to_string())
}

async fn health_check() -> &'static str {
    "OK"
}

pub async fn run(config: Config) -> Result<()> {
    let server = Server::new(config);
    server.run().await
}