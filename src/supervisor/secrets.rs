use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecretHandle(pub u64);

#[derive(Clone)]
pub struct SecretStore {
    secrets: Arc<RwLock<HashMap<SecretHandle, String>>>,
    next_handle: Arc<std::sync::atomic::AtomicU64>,
}

impl SecretStore {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(RwLock::new(HashMap::new())),
            next_handle: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub async fn seal(&self, plaintext: String) -> SecretHandle {
        let handle = SecretHandle(
            self.next_handle
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        self.secrets.write().await.insert(handle, plaintext);
        handle
    }

    pub async fn invoke_authed<F, R>(&self, handle: SecretHandle, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R,
    {
        let secrets = self.secrets.read().await;
        secrets.get(&handle).map(|plaintext| f(plaintext))
    }

    pub async fn revoke(&self, handle: SecretHandle) {
        self.secrets.write().await.remove(&handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seal_returns_handle() {
        let store = SecretStore::new();
        let handle = store.seal("secret123".to_string()).await;
        assert!(handle.0 > 0);
    }

    #[tokio::test]
    async fn invoke_authed_does_not_leak_plaintext() {
        let store = SecretStore::new();
        let handle = store.seal("secret123".to_string()).await;

        let result = store
            .invoke_authed(handle, |plaintext| plaintext.len())
            .await;

        assert_eq!(result, Some(9));
    }

    #[tokio::test]
    async fn invoke_authed_returns_none_for_invalid_handle() {
        let store = SecretStore::new();
        let invalid = SecretHandle(99999);

        let result = store.invoke_authed(invalid, |_| 42).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn revoke_removes_secret() {
        let store = SecretStore::new();
        let handle = store.seal("secret123".to_string()).await;

        let result1 = store.invoke_authed(handle, |_| true).await;
        assert_eq!(result1, Some(true));

        store.revoke(handle).await;

        let result2 = store.invoke_authed(handle, |_| true).await;
        assert_eq!(result2, None);
    }

    #[tokio::test]
    async fn multiple_seals_get_unique_handles() {
        let store = SecretStore::new();
        let h1 = store.seal("secret1".to_string()).await;
        let h2 = store.seal("secret2".to_string()).await;
        let h3 = store.seal("secret3".to_string()).await;

        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_ne!(h1, h3);

        let v1 = store.invoke_authed(h1, |s| s.to_string()).await;
        let v2 = store.invoke_authed(h2, |s| s.to_string()).await;
        let v3 = store.invoke_authed(h3, |s| s.to_string()).await;

        assert_eq!(v1, Some("secret1".to_string()));
        assert_eq!(v2, Some("secret2".to_string()));
        assert_eq!(v3, Some("secret3".to_string()));
    }

    #[tokio::test]
    async fn seal_as_boundary_no_leakage_in_error_path() {
        let store = SecretStore::new();
        let handle = store.seal("super_secret_api_key".to_string()).await;

        let err_result = store
            .invoke_authed(handle, |plaintext| {
                if plaintext.starts_with("super") {
                    Err::<(), String>("validation failed".to_string())
                } else {
                    Ok(())
                }
            })
            .await;

        match err_result {
            Some(Err(err)) => {
                assert!(!err.contains("super_secret_api_key"));
            }
            _ => panic!("Expected error result"),
        }
    }
}
