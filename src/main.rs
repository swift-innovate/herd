use clap::Parser;
use herd::config::Config;
use herd::server;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "herd")]
#[command(about = "Intelligent Ollama router with GPU awareness", long_about = None)]
struct Cli {
    /// Path to config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Port to listen on
    #[arg(short, long, default_value = "40114")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Backend URLs (format: name=url:priority)
    #[arg(short, long)]
    backend: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "herd=info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    let config = if let Some(config_path) = cli.config {
        Config::from_file(&config_path)?
    } else {
        let mut config = Config::default();
        config.server.host = cli.host;
        config.server.port = cli.port;

        for backend in cli.backend {
            let parts: Vec<&str> = backend.split('=').collect();
            if parts.len() == 2 {
                let name = parts[0].to_string();
                let url_parts: Vec<&str> = parts[1].split(':').collect();
                if url_parts.len() >= 3 {
                    let url = format!("http://{}:{}", url_parts[0], url_parts[1]);
                    let priority: u32 = url_parts[2].parse().unwrap_or(50);
                    config.backends.push(herd::config::Backend {
                        name,
                        url,
                        priority,
                        ..Default::default()
                    });
                }
            }
        }

        config
    };

    tracing::info!(
        "Starting Herd on {}:{} with {} backends",
        config.server.host,
        config.server.port,
        config.backends.len()
    );

    server::run(config).await
}