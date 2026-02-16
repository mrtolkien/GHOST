use std::sync::{Mutex, OnceLock};

use chrono::{Duration, Utc};
use ghost::auth::openai_oauth::{StoredTokens, TokenStore};
use ghost::config::CONFIG_DIR_ENV;
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test]
async fn token_store_round_trip_and_revoke() {
    let temp = TempDir::new().expect("tempdir");
    let path = temp.path().join("tokens").join("openai.json");
    let store = TokenStore::new(path.clone());
    let tokens = StoredTokens {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        expires_at: Utc::now() + Duration::hours(1),
    };

    store.save(&tokens).await.expect("save token");
    let loaded = store.current().await.expect("load current").expect("token");
    assert_eq!(loaded.access_token, "access");
    assert_eq!(loaded.refresh_token, "refresh");
    assert!(path.exists());

    store.revoke().await.expect("revoke token");
    assert!(!path.exists());
    assert!(store.current().await.expect("current").is_none());
}

#[test]
fn default_path_respects_config_dir_env() {
    let _guard = env_lock().lock().expect("env lock");
    let temp = TempDir::new().expect("tempdir");

    // SAFETY: test-level synchronization via env_lock prevents concurrent mutation.
    unsafe {
        std::env::set_var(CONFIG_DIR_ENV, temp.path());
    }
    let path = TokenStore::default_openai_path().expect("default path");
    assert_eq!(path, temp.path().join("tokens").join("openai.json"));
    // SAFETY: test-level synchronization via env_lock prevents concurrent mutation.
    unsafe {
        std::env::remove_var(CONFIG_DIR_ENV);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn token_file_permissions_are_600() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().expect("tempdir");
    let path = temp.path().join("tokens").join("openai.json");
    let store = TokenStore::new(path.clone());
    let tokens = StoredTokens {
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        expires_at: Utc::now() + Duration::hours(1),
    };

    store.save(&tokens).await.expect("save token");
    let metadata = std::fs::metadata(path).expect("metadata");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}
