//! Ollama blob extraction — resolve GGUF model files from Ollama's blob store.
//!
//! Ollama stores models in a content-addressed blob store under `~/.ollama/models/`.
//! This module reads the manifest JSON to locate the raw GGUF blob, enabling reuse
//! with llama-server without re-downloading.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Ollama manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OllamaManifest {
    #[serde(default)]
    layers: Vec<OllamaLayer>,
}

#[derive(Debug, Deserialize)]
struct OllamaLayer {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: u64,
}

const MODEL_MEDIA_TYPE: &str = "application/vnd.ollama.image.model";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Information about an extracted Ollama model blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedModel {
    pub model: String,
    pub tag: String,
    pub blob_path: PathBuf,
    pub size_bytes: u64,
    pub digest: String,
}

// ---------------------------------------------------------------------------
// Directory resolution
// ---------------------------------------------------------------------------

/// Find the Ollama models directory.
///
/// Checks `OLLAMA_MODELS` env var first, then falls back to `~/.ollama/models`.
pub fn ollama_models_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("OLLAMA_MODELS") {
        return Ok(PathBuf::from(dir));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(home.join(".ollama").join("models"))
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

/// Parse a manifest file and find the model layer.
fn parse_manifest(data: &[u8]) -> Result<OllamaManifest> {
    serde_json::from_slice(data).context("Invalid Ollama manifest JSON")
}

/// Find the model layer (GGUF blob) in a manifest.
fn find_model_layer(manifest: &OllamaManifest) -> Result<&OllamaLayer> {
    manifest
        .layers
        .iter()
        .find(|l| l.media_type == MODEL_MEDIA_TYPE)
        .ok_or_else(|| anyhow::anyhow!("No model layer found in manifest"))
}

/// Convert a digest like `sha256:abc123` to the blob filename `sha256-abc123`.
fn digest_to_blob_name(digest: &str) -> String {
    digest.replacen(':', "-", 1)
}

/// Resolve a model name and tag to its GGUF blob path.
///
/// Reads the manifest at `manifests/registry.ollama.ai/library/{model}/{tag}`,
/// finds the model layer, and verifies the blob file exists.
pub fn resolve_blob_path(model: &str, tag: &str) -> Result<ExtractedModel> {
    let models_dir = ollama_models_dir()?;
    resolve_blob_path_in(model, tag, &models_dir)
}

/// Resolve a blob path within a specific models directory (testable).
fn resolve_blob_path_in(model: &str, tag: &str, models_dir: &Path) -> Result<ExtractedModel> {
    let manifest_path = models_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join(model)
        .join(tag);

    let data = std::fs::read(&manifest_path)
        .with_context(|| format!("Cannot read manifest for {}:{} at {}", model, tag, manifest_path.display()))?;

    let manifest = parse_manifest(&data)?;
    let layer = find_model_layer(&manifest)?;

    let blob_name = digest_to_blob_name(&layer.digest);
    let blob_path = models_dir.join("blobs").join(&blob_name);

    if !blob_path.exists() {
        anyhow::bail!(
            "Blob file not found: {} (digest {})",
            blob_path.display(),
            layer.digest
        );
    }

    Ok(ExtractedModel {
        model: model.to_string(),
        tag: tag.to_string(),
        blob_path,
        size_bytes: layer.size,
        digest: layer.digest.clone(),
    })
}

/// List all locally available Ollama models with their GGUF blob info.
///
/// Returns an empty vec (not an error) if the Ollama directory is missing.
pub fn list_ollama_models() -> Result<Vec<ExtractedModel>> {
    let models_dir = ollama_models_dir()?;
    list_ollama_models_in(&models_dir)
}

/// List models within a specific models directory (testable).
fn list_ollama_models_in(models_dir: &Path) -> Result<Vec<ExtractedModel>> {
    let library_dir = models_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library");

    if !library_dir.exists() {
        return Ok(vec![]);
    }

    let mut results = Vec::new();

    let model_dirs = std::fs::read_dir(&library_dir)
        .with_context(|| format!("Cannot read {}", library_dir.display()))?;

    for model_entry in model_dirs {
        let model_entry = match model_entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !model_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let model_name = model_entry.file_name().to_string_lossy().to_string();

        let tag_entries = match std::fs::read_dir(model_entry.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for tag_entry in tag_entries {
            let tag_entry = match tag_entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !tag_entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let tag = tag_entry.file_name().to_string_lossy().to_string();

            match resolve_blob_path_in(&model_name, &tag, models_dir) {
                Ok(extracted) => results.push(extracted),
                Err(e) => {
                    tracing::debug!(
                        "Skipping {}:{} — {}",
                        model_name,
                        tag,
                        e
                    );
                }
            }
        }
    }

    Ok(results)
}

/// Copy or symlink a blob to a target path for llama-server use.
///
/// On Unix: tries symlink first, falls back to copy.
/// On Windows: always copies (symlinks require admin privileges).
pub fn extract_to(model: &str, tag: &str, target: &Path) -> Result<ExtractedModel> {
    let resolved = resolve_blob_path(model, tag)?;

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(&resolved.blob_path, target).is_err() {
            std::fs::copy(&resolved.blob_path, target).with_context(|| {
                format!(
                    "Failed to copy blob {} to {}",
                    resolved.blob_path.display(),
                    target.display()
                )
            })?;
        }
    }

    #[cfg(windows)]
    {
        std::fs::copy(&resolved.blob_path, target).with_context(|| {
            format!(
                "Failed to copy blob {} to {}",
                resolved.blob_path.display(),
                target.display()
            )
        })?;
    }

    Ok(resolved)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SAMPLE_MANIFEST: &str = r#"{
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "digest": "sha256:configabc",
            "size": 123
        },
        "layers": [
            {
                "mediaType": "application/vnd.ollama.image.model",
                "digest": "sha256:abc123def456",
                "size": 15700000000
            },
            {
                "mediaType": "application/vnd.ollama.image.template",
                "digest": "sha256:template789",
                "size": 1234
            }
        ]
    }"#;

    const MANIFEST_NO_MODEL: &str = r#"{
        "schemaVersion": 2,
        "layers": [
            {
                "mediaType": "application/vnd.ollama.image.template",
                "digest": "sha256:template789",
                "size": 1234
            }
        ]
    }"#;

    /// Create a unique temp directory for tests and return its path.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herd-blob-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_manifest_finds_model_layer() {
        let manifest = parse_manifest(SAMPLE_MANIFEST.as_bytes()).unwrap();
        let layer = find_model_layer(&manifest).unwrap();
        assert_eq!(layer.media_type, MODEL_MEDIA_TYPE);
        assert_eq!(layer.digest, "sha256:abc123def456");
        assert_eq!(layer.size, 15_700_000_000);
    }

    #[test]
    fn digest_to_blob_name_replaces_colon() {
        assert_eq!(
            digest_to_blob_name("sha256:abc123"),
            "sha256-abc123"
        );
    }

    #[test]
    fn digest_to_blob_name_only_first_colon() {
        assert_eq!(
            digest_to_blob_name("sha256:abc:extra"),
            "sha256-abc:extra"
        );
    }

    #[test]
    fn missing_model_layer_returns_error() {
        let manifest = parse_manifest(MANIFEST_NO_MODEL.as_bytes()).unwrap();
        let err = find_model_layer(&manifest).unwrap_err();
        assert!(err.to_string().contains("No model layer"));
    }

    #[test]
    fn invalid_manifest_json_returns_error() {
        let result = parse_manifest(b"not json at all");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid Ollama manifest"));
    }

    #[test]
    fn resolve_blob_path_nonexistent_model() {
        let tmp = test_dir("resolve-noexist");
        let err = resolve_blob_path_in("nomodel", "latest", &tmp).unwrap_err();
        assert!(err.to_string().contains("Cannot read manifest"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_blob_path_success() {
        let models_dir = test_dir("resolve-ok");

        // Create manifest directory structure
        let manifest_dir = models_dir
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join("llama3");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(manifest_dir.join("8b"), SAMPLE_MANIFEST).unwrap();

        // Create blob file
        let blobs_dir = models_dir.join("blobs");
        fs::create_dir_all(&blobs_dir).unwrap();
        fs::write(blobs_dir.join("sha256-abc123def456"), b"fake gguf data").unwrap();

        let result = resolve_blob_path_in("llama3", "8b", &models_dir).unwrap();
        assert_eq!(result.model, "llama3");
        assert_eq!(result.tag, "8b");
        assert_eq!(result.size_bytes, 15_700_000_000);
        assert_eq!(result.digest, "sha256:abc123def456");
        assert!(result.blob_path.ends_with("sha256-abc123def456"));

        let _ = fs::remove_dir_all(&models_dir);
    }

    #[test]
    fn resolve_blob_path_missing_blob_file() {
        let models_dir = test_dir("resolve-noblob");

        // Create manifest but no blob
        let manifest_dir = models_dir
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join("llama3");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(manifest_dir.join("8b"), SAMPLE_MANIFEST).unwrap();

        let err = resolve_blob_path_in("llama3", "8b", &models_dir).unwrap_err();
        assert!(err.to_string().contains("Blob file not found"));

        let _ = fs::remove_dir_all(&models_dir);
    }

    #[test]
    fn list_models_empty_on_missing_dir() {
        let tmp = test_dir("list-empty");
        let result = list_ollama_models_in(&tmp).unwrap();
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_models_finds_installed_models() {
        let models_dir = test_dir("list-found");

        // Create two models
        for (model, tag, digest) in &[
            ("llama3", "8b", "sha256-aaa111"),
            ("mistral", "latest", "sha256-bbb222"),
        ] {
            let manifest_dir = models_dir
                .join("manifests")
                .join("registry.ollama.ai")
                .join("library")
                .join(model);
            fs::create_dir_all(&manifest_dir).unwrap();

            let manifest = format!(
                r#"{{
                    "schemaVersion": 2,
                    "layers": [{{
                        "mediaType": "application/vnd.ollama.image.model",
                        "digest": "{}",
                        "size": 1000
                    }}]
                }}"#,
                digest.replace('-', ":")
            );
            fs::write(manifest_dir.join(tag), manifest).unwrap();

            let blobs_dir = models_dir.join("blobs");
            fs::create_dir_all(&blobs_dir).unwrap();
            fs::write(blobs_dir.join(digest), b"fake").unwrap();
        }

        let models = list_ollama_models_in(&models_dir).unwrap();
        assert_eq!(models.len(), 2);

        let names: Vec<_> = models.iter().map(|m| m.model.as_str()).collect();
        assert!(names.contains(&"llama3"));
        assert!(names.contains(&"mistral"));

        let _ = fs::remove_dir_all(&models_dir);
    }
}
