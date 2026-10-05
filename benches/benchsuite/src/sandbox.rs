//! Isolated filesystem state for benchmark workloads.

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tempfile::{Builder, TempDir};

use crate::fixture::{Fixture, StagedFixture};

#[derive(Debug)]
pub struct Sandbox {
    root: TempDir,
    config_dir: PathBuf,
    cache_dir: PathBuf,
}

impl Sandbox {
    pub fn new() -> Result<Self> {
        let root = Builder::new()
            .prefix("symposium-benchmark-")
            .tempdir()
            .context("creating benchmark sandbox")?;
        let config_dir = root.path().join("symposium-home");
        let cache_dir = config_dir.join("cache");

        fs::create_dir_all(&cache_dir).with_context(|| {
            format!(
                "creating benchmark sandbox directories under `{}`",
                root.path().display()
            )
        })?;

        Ok(Self {
            root,
            config_dir,
            cache_dir,
        })
    }

    /// Copy `fixture` into the sandbox and validate its layout.
    pub fn stage(&self, fixture: Fixture) -> Result<StagedFixture> {
        StagedFixture::stage(fixture, self.root().join(fixture.directory_name()))
    }

    /// Write the sandbox configuration, refusing to replace an existing file.
    pub fn write_config(&self, contents: &str) -> Result<()> {
        let path = self.config_dir.join("config.toml");
        let mut file = File::create_new(&path)
            .with_context(|| format!("creating benchmark configuration `{}`", path.display()))?;

        file.write_all(contents.as_bytes())
            .with_context(|| format!("writing benchmark configuration `{}`", path.display()))
    }

    /// Remove the sandbox's workspace dependency caches.
    pub fn clear_workspace_cache(&self) -> Result<()> {
        let workspace_cache = self.cache_dir.join("workspaces");

        match fs::remove_dir_all(&workspace_cache) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| {
                format!(
                    "removing benchmark workspace cache `{}`",
                    workspace_cache.display()
                )
            }),
        }
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_isolated_sandbox_directories() -> Result<()> {
        let sandbox = Sandbox::new()?;

        assert!(sandbox.root().is_dir());
        assert!(sandbox.config_dir().is_dir());
        assert!(sandbox.cache_dir().is_dir());
        assert_eq!(sandbox.config_dir().parent(), Some(sandbox.root()));
        assert_eq!(sandbox.cache_dir().parent(), Some(sandbox.config_dir()));

        Ok(())
    }

    #[test]
    fn stages_only_the_requested_fixture() -> Result<()> {
        let sandbox = Sandbox::new()?;

        let project = sandbox.stage(Fixture::ReferenceProject)?;

        assert_eq!(project.path(), sandbox.root().join("reference-project"));
        assert_eq!(project.fixture(), Fixture::ReferenceProject);
        assert!(!sandbox.root().join("local-registry").try_exists()?);

        Ok(())
    }

    #[test]
    fn refuses_to_stage_the_same_fixture_twice() -> Result<()> {
        let sandbox = Sandbox::new()?;

        sandbox.stage(Fixture::LocalRegistry)?;
        sandbox
            .stage(Fixture::LocalRegistry)
            .expect_err("staging the same fixture twice must fail");

        Ok(())
    }

    #[test]
    fn writes_configuration_contents() -> Result<()> {
        let sandbox = Sandbox::new()?;

        sandbox.write_config("benchmark configuration")?;

        assert_eq!(
            fs::read_to_string(sandbox.config_dir().join("config.toml"))?,
            "benchmark configuration"
        );

        Ok(())
    }

    #[test]
    fn refuses_to_overwrite_configuration() -> Result<()> {
        let sandbox = Sandbox::new()?;
        let config_file = sandbox.config_dir().join("config.toml");
        sandbox.write_config("original configuration")?;

        let error = sandbox
            .write_config("replacement configuration")
            .expect_err("writing configuration twice must fail");

        assert!(
            error
                .to_string()
                .contains(&config_file.display().to_string()),
            "error should identify the existing configuration: {error:#}"
        );
        assert_eq!(fs::read_to_string(config_file)?, "original configuration");

        Ok(())
    }

    #[test]
    fn clears_only_the_workspace_cache() -> Result<()> {
        let sandbox = Sandbox::new()?;
        let project = sandbox.stage(Fixture::ReferenceProject)?;
        let config_file = sandbox.config_dir().join("config.toml");
        let workspace_cache = sandbox
            .cache_dir()
            .join("workspaces")
            .join("reference-project");
        let binary_cache = sandbox
            .cache_dir()
            .join("binaries")
            .join("example")
            .join("1.0.0");

        fs::write(&config_file, "benchmark configuration")?;
        fs::create_dir_all(&workspace_cache)?;
        fs::write(workspace_cache.join("workspace-deps.json"), "cached data")?;
        fs::create_dir_all(&binary_cache)?;
        fs::write(binary_cache.join("example"), "cached binary")?;

        sandbox.clear_workspace_cache()?;
        sandbox.clear_workspace_cache()?;

        assert!(sandbox.cache_dir().is_dir());
        assert!(!workspace_cache.try_exists()?);
        assert_eq!(
            fs::read_to_string(binary_cache.join("example"))?,
            "cached binary"
        );
        assert!(project.path().join("Cargo.toml").is_file());
        assert_eq!(fs::read_to_string(config_file)?, "benchmark configuration");

        Ok(())
    }
}
