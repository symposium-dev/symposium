//! The `claude` to stage, as it is installed: a native install is one executable, and an
//! npm install is a link to a Node script inside its package, staged whole with the
//! `node` that runs it.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use super::host;
use crate::run::stage::Tool;

/// What staging `claude` takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ClaudeTools {
    pub(crate) claude: Tool,
    /// The `node` an npm install runs through.
    pub(crate) node: Option<Tool>,
}

/// The `claude` on `path`, as it was installed.
pub(crate) fn claude_tools(path: &OsStr) -> anyhow::Result<ClaudeTools> {
    let installed = host::resolved(&host::require(path, "claude")?)?;
    if !is_node_script(&installed)? {
        return Ok(ClaudeTools {
            claude: Tool::Executable(installed),
            node: None,
        });
    }
    let Some(package) = package_root(&installed) else {
        bail!(
            "`{}` is a Node script outside any package",
            installed.display()
        );
    };
    let entry = installed
        .strip_prefix(&package)
        .expect("the package holds the script")
        .to_owned();
    Ok(ClaudeTools {
        claude: Tool::NodePackage { package, entry },
        node: Some(Tool::Executable(host::require(path, "node")?)),
    })
}

/// Whether `file` starts with a `#!` line that runs `node`.
fn is_node_script(file: &Path) -> anyhow::Result<bool> {
    let mut start = Vec::new();
    File::open(file)
        .and_then(|opened| opened.take(256).read_to_end(&mut start))
        .with_context(|| format!("cannot read `{}`", file.display()))?;
    let first_line = start
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    Ok(first_line.starts_with(b"#!") && String::from_utf8_lossy(first_line).contains("node"))
}

/// The nearest directory above `file` with a `package.json`.
fn package_root(file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .find(|dir| dir.join("package.json").is_file())
        .map(Path::to_owned)
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;
    use crate::run::test_support::executable;

    /// A Claude Code installation laid out as the production installers lay it out.
    struct ClaudeInstall {
        _temp: TempDir,
        bin: PathBuf,
        target: PathBuf,
        package: Option<PathBuf>,
    }

    impl ClaudeInstall {
        fn native() -> Self {
            let temp = TempDir::new().unwrap();
            let target = temp.path().join("versions/2.1.318");
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, b"\x7fELF").unwrap();
            Self::link(temp, target, None)
        }

        fn npm() -> Self {
            let temp = TempDir::new().unwrap();
            let package = temp
                .path()
                .join("lib/node_modules/@anthropic-ai/claude-code");
            fs::create_dir_all(&package).unwrap();
            fs::write(package.join("package.json"), "{}").unwrap();
            let target = package.join("cli.js");
            executable(&target, "#!/usr/bin/env node\n");
            Self::link(temp, target, Some(package))
        }

        fn unpackaged_node_script() -> Self {
            let temp = TempDir::new().unwrap();
            let target = temp.path().join("claude.js");
            executable(&target, "#!/usr/bin/env node\n");
            Self::link(temp, target, None)
        }

        fn link(temp: TempDir, target: PathBuf, package: Option<PathBuf>) -> Self {
            let bin = temp.path().join("bin");
            fs::create_dir(&bin).unwrap();
            symlink(&target, bin.join("claude")).unwrap();
            executable(&bin.join("node"), "#!/bin/sh\n");
            Self {
                _temp: temp,
                bin,
                target,
                package,
            }
        }

        fn path(&self) -> &OsStr {
            self.bin.as_os_str()
        }

        fn resolved_target(&self) -> PathBuf {
            fs::canonicalize(&self.target).unwrap()
        }

        fn resolved_package(&self) -> PathBuf {
            fs::canonicalize(self.package.as_ref().unwrap()).unwrap()
        }
    }

    #[test]
    fn a_native_install_is_one_executable() {
        let install = ClaudeInstall::native();

        let tools = claude_tools(install.path()).unwrap();

        assert_eq!(
            tools,
            ClaudeTools {
                claude: Tool::Executable(install.resolved_target()),
                node: None
            }
        );
    }

    #[test]
    fn an_npm_install_is_its_package_and_node() {
        let install = ClaudeInstall::npm();

        let tools = claude_tools(install.path()).unwrap();

        assert_eq!(
            tools,
            ClaudeTools {
                claude: Tool::NodePackage {
                    package: install.resolved_package(),
                    entry: "cli.js".into(),
                },
                node: Some(Tool::Executable(install.bin.join("node"))),
            },
        );
    }

    #[test]
    fn a_node_script_outside_a_package_is_refused() {
        let install = ClaudeInstall::unpackaged_node_script();

        assert!(claude_tools(install.path()).is_err());
    }
}
