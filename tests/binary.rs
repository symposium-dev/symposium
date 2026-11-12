use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use symposium::cli::Binary;

const INVOCATIONS: &[&[&str]] = &[
    &["init", "--add-agent", "claude", "--hook-scope", "project"],
    &["sync"],
    &["search", "serde"],
    &["use", "serde", "--global"],
    &["status"],
    &["hook", "claude", "session-start"],
    &["plugin", "list"],
    &["self-update"],
    &["crate-info", "serde", "--version", "1"],
    &["telemetry", "status"],
    &["my-tool", "--flag"],
];

#[test]
fn both_binaries_parse_every_subcommand_the_same() {
    let covered: BTreeSet<_> = INVOCATIONS.iter().map(|args| args[0]).collect();
    for subcommand in Binary::Symposium.command().get_subcommands() {
        assert!(
            covered.contains(subcommand.get_name()),
            "no invocation covers `{}`",
            subcommand.get_name()
        );
    }

    for args in INVOCATIONS {
        let symposium = Binary::Symposium
            .parse(std::iter::once("symposium").chain(args.iter().copied()))
            .unwrap();
        let cargo_agents = Binary::CargoAgents
            .parse(std::iter::once("cargo-agents").chain(args.iter().copied()))
            .unwrap();
        assert_eq!(format!("{symposium:?}"), format!("{cargo_agents:?}"));
    }
}

fn isolated_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".symposium");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "auto-update = \"off\"\n\n[defaults]\nsymposium-recommendations = false\n",
    )
    .unwrap();
    home
}

fn run(home: &Path, binary: Binary, args: &[&str]) -> Output {
    let mut command = match binary {
        Binary::Symposium => Command::new(env!("CARGO_BIN_EXE_symposium")),
        Binary::CargoAgents => {
            let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-agents"));
            command.arg("agents");
            command
        }
    };
    let output = command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("SYMPOSIUM_HOME", home.join(".symposium"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {args:?}: {}",
        binary.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn help_names_the_invoked_binary() {
    let home = isolated_home();
    for args in [
        &["--help"][..],
        &["sync", "--help"],
        &["plugin", "list", "-h"],
    ] {
        let symposium = stdout(&run(home.path(), Binary::Symposium, args));
        let cargo_agents = stdout(&run(home.path(), Binary::CargoAgents, args));

        assert!(symposium.contains("Usage: symposium"), "{symposium}");
        assert!(
            cargo_agents.contains("Usage: cargo agents"),
            "{cargo_agents}"
        );
        assert_eq!(
            symposium.replace("Usage: symposium", "Usage: cargo agents"),
            cargo_agents
        );
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == "logs") {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[cfg(unix)]
#[test]
fn symposium_init_after_cargo_agents_init_changes_nothing() {
    let home = isolated_home();
    let args = ["init", "--add-agent", "claude"];

    let first = stdout(&run(home.path(), Binary::CargoAgents, &args));
    assert!(first.contains("added hooks"), "{first}");
    let after_first = snapshot(home.path());

    let second = stdout(&run(home.path(), Binary::Symposium, &args));
    assert!(second.contains("hooks already registered"), "{second}");
    assert_eq!(after_first, snapshot(home.path()));
}
