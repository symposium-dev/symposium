use std::path::{Path, PathBuf};

/// Canonical locations used by local telemetry storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoragePaths {
    telemetry_dir: PathBuf,
    lock_file: PathBuf,
    state_file: PathBuf,
}

impl StoragePaths {
    #[must_use]
    pub(super) fn new(config_dir: &Path) -> Self {
        let telemetry_dir = config_dir.join("telemetry");
        let lock_file = telemetry_dir.join(".lock");
        let state_file = config_dir.join("telemetry-state.toml");

        Self {
            telemetry_dir,
            lock_file,
            state_file,
        }
    }

    #[must_use]
    pub(super) fn telemetry_dir(&self) -> &Path {
        &self.telemetry_dir
    }

    #[must_use]
    pub(super) fn lock_file(&self) -> &Path {
        &self.lock_file
    }

    #[must_use]
    pub(super) fn state_file(&self) -> &Path {
        &self.state_file
    }
}

#[cfg(test)]
mod tests {
    use super::StoragePaths;

    #[test]
    fn storage_paths_keep_private_state_outside_inspectable_data() {
        let temporary = tempfile::tempdir().unwrap();

        let paths = StoragePaths::new(temporary.path());

        assert_eq!(paths.telemetry_dir(), temporary.path().join("telemetry"));
        assert_eq!(
            paths.lock_file(),
            temporary.path().join("telemetry").join(".lock")
        );
        assert_eq!(
            paths.state_file(),
            temporary.path().join("telemetry-state.toml")
        );
    }
}
