//! Where a remote server's OAuth tokens live between sessions.
//!
//! One file per server under `<config>/credentials/`, mode `0600`. A file per
//! server rather than one combined store so that revoking a single server is a
//! delete, and so a corrupt file cannot cost the others.
//!
//! The SDK ships an in-memory store, which would mean logging in again every
//! session. It writes through this trait on every refresh, so persistence is
//! the only thing added here.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};

pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    /// Store for one server, named for it.
    pub fn new(config_dir: &Path, server: &str) -> Self {
        Self {
            path: credentials_dir(config_dir).join(format!("{}.json", sanitize(server))),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

pub fn credentials_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("credentials")
}

/// A server name reaches this from a manifest, so it must not be able to
/// escape the credentials directory or collide by punctuation alone.
fn sanitize(server: &str) -> String {
    server
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(AuthError::InternalError(e.to_string())),
        };
        // A credential file that no longer parses is treated as absent: the
        // recovery is to log in again, which is what a caller does with `None`.
        match serde_json::from_slice(&bytes) {
            Ok(stored) => Ok(Some(stored)),
            Err(e) => {
                tracing::warn!(path = %self.path.display(), error = %e, "ignoring unreadable credentials");
                Ok(None)
            }
        }
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| AuthError::InternalError("credential path has no parent".into()))?;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| AuthError::InternalError(e.to_string()))?;
        restrict(dir).await?;

        let json = serde_json::to_vec_pretty(&credentials)
            .map_err(|e| AuthError::InternalError(e.to_string()))?;

        // Temp file plus rename, so a refresh interrupted midway cannot leave a
        // half-written token behind; the mode is set before the contents are
        // visible under the final name.
        let temp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        tokio::fs::write(&temp, &json)
            .await
            .map_err(|e| AuthError::InternalError(e.to_string()))?;
        restrict(&temp).await?;
        if let Err(e) = tokio::fs::rename(&temp, &self.path).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(AuthError::InternalError(e.to_string()));
        }
        Ok(())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AuthError::InternalError(e.to_string())),
        }
    }
}

#[cfg(unix)]
async fn restrict(path: &Path) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .await
        .map_err(|e| AuthError::InternalError(e.to_string()))
}

#[cfg(not(unix))]
async fn restrict(_path: &Path) -> Result<(), AuthError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(token: &str) -> StoredCredentials {
        let response = serde_json::from_value(serde_json::json!({
            "access_token": token,
            "token_type": "bearer",
        }))
        .expect("token response");
        StoredCredentials::new("client-1".to_string(), Some(response), Vec::new(), None)
    }

    #[tokio::test]
    async fn a_saved_token_is_read_back() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = FileCredentialStore::new(tmp.path(), "sentry");

        assert!(store.load().await.expect("load").is_none());
        store.save(stored("abc")).await.expect("save");

        let loaded = store.load().await.expect("load").expect("some");
        assert_eq!(loaded.client_id, "client-1");
    }

    #[tokio::test]
    async fn credentials_are_not_world_readable() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = FileCredentialStore::new(tmp.path(), "sentry");
        store.save(stored("abc")).await.expect("save");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.path())
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
        }
    }

    #[tokio::test]
    async fn clearing_removes_the_file_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = FileCredentialStore::new(tmp.path(), "sentry");
        store.save(stored("abc")).await.expect("save");

        store.clear().await.expect("clear");
        assert!(!store.exists());
        store.clear().await.expect("clearing twice is not an error");
    }

    /// A server name comes from a manifest, so it must not be able to write
    /// outside the credentials directory.
    #[tokio::test]
    async fn a_traversing_name_stays_inside_the_directory() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = FileCredentialStore::new(tmp.path(), "../../evil");
        store.save(stored("abc")).await.expect("save");

        assert_eq!(
            store.path().parent(),
            Some(credentials_dir(tmp.path()).as_path())
        );
        assert!(store.exists());
    }

    /// Recovery from a corrupt file is logging in again, which is what a
    /// caller does when the store answers `None`.
    #[tokio::test]
    async fn an_unreadable_file_reads_as_absent() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = FileCredentialStore::new(tmp.path(), "sentry");
        std::fs::create_dir_all(credentials_dir(tmp.path())).expect("dir");
        std::fs::write(store.path(), b"not json").expect("write");

        assert!(store.load().await.expect("load").is_none());
    }
}
