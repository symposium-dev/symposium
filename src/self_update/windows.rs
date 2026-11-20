//! Windows can rename a running executable, but cannot overwrite or delete it.
//!
//! Keep the old image beside its original path until a later invocation can
//! delete it. In particular, do not detach the update: its exit status belongs
//! to the caller, and an automatic update must preserve the command's I/O.

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(super) struct ExecutableBackup {
    original: PathBuf,
    backup: PathBuf,
}

impl ExecutableBackup {
    pub(super) fn prepare() -> Result<Option<Self>> {
        let original =
            std::env::current_exe().context("could not locate the running executable")?;
        // The library API can also run inside another application's process.
        // Cargo installs only these two CLI executables; never rename a host.
        if !is_cli_executable(&original) {
            return Ok(None);
        }
        Self::prepare_at(original).map(Some)
    }

    fn prepare_at(original: PathBuf) -> Result<Self> {
        // Complete and flush the copy before moving the installed command.
        // Cancellation while copying can then leave only a private staging
        // file, never a partially written public executable.
        let staged = stage_executable(&original)?;
        Self::activate(original, &staged)
    }

    fn activate(original: PathBuf, staged: &Path) -> Result<Self> {
        let backup = backup_path(&original);
        remove_backup(&backup).with_context(|| {
            format!(
                "could not remove previous update backup {}; another Symposium process may still be running",
                backup.display()
            )
        })?;
        std::fs::rename(&original, &backup).with_context(|| {
            format!(
                "could not back up running executable {}",
                original.display()
            )
        })?;
        let saved = Self { original, backup };
        // Each rename is atomic, but the pair is not. If the process is killed
        // between them, the complete previous image remains at `backup` for
        // manual recovery. A normal activation error rolls back immediately.
        if let Err(error) = std::fs::rename(staged, &saved.original) {
            let error =
                anyhow::Error::new(error).context("could not activate the staged executable");
            return Err(saved.restore_after_failure(error));
        }
        Ok(saved)
    }

    fn restore(self) -> Result<()> {
        std::fs::rename(&self.backup, &self.original).with_context(|| {
            format!(
                "could not restore {} from {}",
                self.original.display(),
                self.backup.display()
            )
        })
    }

    pub(super) fn restore_after_failure(self, error: anyhow::Error) -> anyhow::Error {
        match self.restore() {
            Ok(()) => error,
            Err(restore_error) => error.context(format!(
                "also failed to restore the previous executable: {restore_error:#}"
            )),
        }
    }
}

fn stage_executable(original: &Path) -> Result<tempfile::TempPath> {
    let parent = original
        .parent()
        .context("executable has no parent directory")?;
    let staged = tempfile::Builder::new()
        .prefix(".symposium-update-")
        .suffix(".exe")
        .tempfile_in(parent)
        .context("could not create an executable staging file")?;
    std::fs::copy(original, staged.path()).context("could not stage the installed executable")?;
    staged
        .as_file()
        .sync_all()
        .context("could not flush the staged executable")?;
    // Close the staging handle before renaming the image on Windows.
    Ok(staged.into_temp_path())
}

pub(super) fn clean_up_previous_executable() {
    if let Ok(executable) = std::env::current_exe()
        && is_cli_executable(&executable)
    {
        // An auto-update's previous process can still be waiting for this one.
        // Leave its mapped backup alone; a later invocation will retry.
        let _ = remove_backup(&backup_path(&executable));
    }
}

fn is_cli_executable(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case("symposium.exe") || name.eq_ignore_ascii_case("cargo-agents.exe")
    })
}

fn backup_path(executable: &Path) -> PathBuf {
    let mut name = executable.as_os_str().to_owned();
    name.push(".self-update-old");
    name.into()
}

fn remove_backup(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_completes_before_the_installed_command_is_moved() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();

        let staged = stage_executable(&original).unwrap();

        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
        assert_eq!(std::fs::read(&staged).unwrap(), b"previous image");
        assert!(!backup_path(&original).exists());
        let staged_path = staged.to_path_buf();
        drop(staged);
        assert!(!staged_path.exists());
    }

    #[test]
    fn staging_read_failure_leaves_the_installed_command_untouched() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&original)
            .unwrap();

        let result = ExecutableBackup::prepare_at(original.clone());
        drop(locked);

        let Err(error) = result else {
            panic!("staging must fail while reads are denied");
        };
        assert!(error.to_string().contains("could not stage"));
        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
        assert!(!backup_path(&original).exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn activation_failure_after_backup_restores_the_installed_command() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();

        let result =
            ExecutableBackup::activate(original.clone(), &root.path().join("missing-stage"));

        let Err(error) = result else {
            panic!("activation of a missing image must fail");
        };
        assert!(error.to_string().contains("could not activate"));
        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
        assert!(!backup_path(&original).exists());
    }

    #[test]
    fn rollback_failure_preserves_the_backup_and_both_errors() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();
        let backup = ExecutableBackup::prepare_at(original.clone()).unwrap();
        std::fs::remove_file(&original).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("block-restore"), "").unwrap();

        let error = backup.restore_after_failure(anyhow::anyhow!("installation failed"));

        let diagnostic = format!("{error:#}");
        assert!(diagnostic.contains("installation failed"));
        assert!(diagnostic.contains("could not restore"));
        assert!(diagnostic.contains("symposium.exe.self-update-old"));
        assert_eq!(
            std::fs::read(backup_path(&original)).unwrap(),
            b"previous image"
        );
    }

    #[test]
    fn interrupted_activation_leaves_a_complete_image_for_manual_recovery() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();
        let staged = stage_executable(&original).unwrap();
        let backup = backup_path(&original);
        // Model termination at the boundary between the two atomic renames.
        std::fs::rename(&original, &backup).unwrap();
        drop(staged);

        assert!(!original.exists());
        assert_eq!(std::fs::read(&backup).unwrap(), b"previous image");
        std::fs::rename(&backup, &original).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
    }

    #[test]
    fn preparation_keeps_the_installed_command_available() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();

        let backup = ExecutableBackup::prepare_at(original.clone()).unwrap();

        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
        assert_eq!(std::fs::read(&backup.backup).unwrap(), b"previous image");
    }

    #[test]
    fn failure_restores_the_previous_image_after_partial_replacement() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();
        let backup = ExecutableBackup::prepare_at(original.clone()).unwrap();
        std::fs::write(&original, "incomplete update").unwrap();

        let error = backup.restore_after_failure(anyhow::anyhow!("installation failed"));

        assert_eq!(error.to_string(), "installation failed");
        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
        assert!(!backup_path(&original).exists());
    }

    #[test]
    fn cleanup_removes_an_old_image_and_accepts_an_absent_one() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("symposium.exe.self-update-old");
        std::fs::write(&path, "old image").unwrap();

        remove_backup(&path).unwrap();
        remove_backup(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn preparation_failure_leaves_the_installed_command_untouched() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("symposium.exe");
        std::fs::write(&original, "previous image").unwrap();
        std::fs::create_dir(backup_path(&original)).unwrap();

        assert!(ExecutableBackup::prepare_at(original.clone()).is_err());

        assert_eq!(std::fs::read(&original).unwrap(), b"previous image");
    }
}
