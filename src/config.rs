use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    
    #[serde(default)]
    pub routing: RoutingConfig,
    
    #[serde(default)]
    pub backends: Vec<Backend>,
    
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
    
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 40114 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default = "default_strategy")]
    pub strategy: RoutingStrategy,
    
    #[serde(default = "default_timeout")]
    pub timeout: String,
    
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            timeout: default_timeout(),
            retry_count: default_retry_count(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RoutingStrategy {
    #[serde(rename = "priority")]
    Priority,
    
    #[serde(rename = "model_aware")]
    ModelAware,
    
    #[serde(rename = "least_busy")]
    LeastBusy,
}

fn default_strategy() -> RoutingStrategy { RoutingStrategy::Priority }
fn default_timeout() -> String { "120s".to_string() }
fn default_retry_count() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    pub name: String,
    pub url: String,
    pub priority: u32,
    
    #[serde(default)]
    pub gpu_hot_url: Option<String>,
    
    #[serde(default)]
    pub model_filter: Option<String>,
    
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            priority: 50,
            gpu_hot_url: None,
            model_filter: None,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    
    #[serde(default = "default_timeout")]
    pub timeout: String,
    
    #[serde(default = "default_recovery_time")]
    pub recovery_time: String,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: default_failure_threshold(),
            timeout: default_timeout(),
            recovery_time: default_recovery_time(),
        }
    }
}

fn default_failure_threshold() -> u32 { 3 }
fn default_recovery_time() -> String { "60s".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_true")]
    pub metrics: bool,
    
    #[serde(default = "default_true")]
    pub admin_api: bool,
    
    #[serde(default)]
    pub tracing: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            metrics: true,
            admin_api: true,
            tracing: false,
        }
    }
}

fn default_true() -> bool { true }

impl Config {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }
    
    pub fn to_yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            routing: RoutingConfig::default(),
            backends: Vec::new(),
            circuit_breaker: CircuitBreakerConfig::default(),
            observability: ObservabilityConfig::default(),
        }
    }
}