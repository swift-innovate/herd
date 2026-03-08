use anyhow::Result;

const REPO_OWNER: &str = "swift-innovate";
const REPO_NAME: &str = "herd";
const BIN_NAME: &str = "herd";

/// Information about an available update.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
}

/// Check GitHub Releases for a newer version without applying it.
pub fn check_for_update() -> Result<UpdateInfo> {
    let current = self_update::cargo_crate_version!().to_string();

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()?
        .fetch()?;

    let latest = releases
        .first()
        .map(|r| r.version.clone())
        .unwrap_or_else(|| current.clone());

    let update_available = version_is_newer(&current, &latest);

    Ok(UpdateInfo {
        current,
        latest,
        update_available,
    })
}

/// Download and apply the latest release, replacing the current binary.
/// Returns the new version string on success.
pub fn perform_update(show_progress: bool) -> Result<String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .show_download_progress(show_progress)
        .current_version(self_update::cargo_crate_version!())
        .build()?
        .update()?;

    Ok(status.version().to_string())
}

/// Simple semver comparison: returns true if `latest` is newer than `current`.
fn version_is_newer(current: &str, latest: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let c = parse(current);
    let l = parse(latest);
    l > c
}

/// Log an update notification at startup (non-blocking, best-effort).
pub async fn startup_update_check() {
    // Run in a blocking task since self_update uses synchronous HTTP
    let result = tokio::task::spawn_blocking(check_for_update).await;

    match result {
        Ok(Ok(info)) if info.update_available => {
            tracing::info!(
                "Update available: v{} → v{} (run `herd --update` to install)",
                info.current,
                info.latest
            );
        }
        Ok(Ok(_)) => {
            tracing::debug!("Herd is up to date");
        }
        Ok(Err(e)) => {
            tracing::debug!("Update check failed: {}", e);
        }
        Err(e) => {
            tracing::debug!("Update check task failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_newer() {
        assert!(version_is_newer("0.3.0", "0.4.0"));
        assert!(version_is_newer("0.4.0", "1.0.0"));
        assert!(version_is_newer("0.4.0", "0.4.1"));
    }

    #[test]
    fn version_comparison_same_or_older() {
        assert!(!version_is_newer("0.4.0", "0.4.0"));
        assert!(!version_is_newer("1.0.0", "0.4.0"));
        assert!(!version_is_newer("0.4.1", "0.4.0"));
    }

    #[test]
    fn version_comparison_handles_v_prefix() {
        assert!(version_is_newer("v0.3.0", "v0.4.0"));
        assert!(!version_is_newer("v0.4.0", "0.4.0"));
    }
}
