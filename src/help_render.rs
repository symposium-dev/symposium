//! Custom `cargo agents --help` renderer.
//!
//! clap's auto help flag and help subcommand are disabled (see[`crate::cli::Cli`]), so top-level `--help` / `-h` / `help` / no-subcommand invocations land here *after* parsing and get an audience-grouped help text per `md/design/subcommands.md`.
//!
//! The header and options block are sliced out of clap's own rendered help, so the only hand-coded strings are the two section headings.
//! If clap's format ever drifts and a slice marker isn't found, we fall back to clap's unmodified help rather than panic.
//!

use std::{fmt::Write as _, path::Path};

use clap::{Command, CommandFactory};
use semver::Version;

use symposium_sdk::workspace::WorkspaceCrate;

use crate::{
    cli::{Cli, Commands, builtin_audience},
    config::Symposium,
    plugins::{Audience, PluginRegistry, load_registry},
    subcommand_dispatch::applicable_subcommands,
};

/// Section headings for the audience-grouped help. Referenced by the
/// SessionStart discovery hint (`crate::hook`) too, so the rendered help and
/// the prompt that points agents at it can't drift apart.
pub const HUMANS_HEADING: &str = "Commands for humans";
pub const AGENTS_HEADING: &str = "Commands for agents";

/// If this invocation is a help request, return the help text to print.
///
/// Returns `None` when the user wants to actually run something.
///
/// - no subcommand, or the bare `help` keyword -> top-level
/// - `<built-in> --help` (incl. nested and required-arg commands) -> clap's own per command help, re-rendered;
/// - a plugin-vended `<name> --help` -> `None`, so dispatch forwards `--help` to the child.
pub fn help_text(
    parse: Result<&Cli, &clap::Error>,
    args: &[String],
    sym: &Symposium,
    cwd: &Path,
) -> Option<String> {
    match parse {
        Ok(cli) => {
            let help_keyword = matches!(&cli.command, Some(Commands::External(argv)) if argv.first().and_then(|fst| fst.to_str()) == Some("help"));

            if cli.command.is_none() || help_keyword {
                return Some(render_help(sym, cwd));
            }

            if cli.help {
                // `<built-in> --help`; fall back to top-level if the target is a plugin (External) or `--help` come before any subcommand name.
                return Some(subcommand_help(args).unwrap_or_else(|| render_help(sym, cwd)));
            }

            None
        }
        // Parse failed. If help was requested on a built-in with required args/subcommands, recover by rendering that command's help; otherwise it's a genuine error and the caller let's clap report it.
        Err(_) => {
            let asked = args.iter().any(|arg| arg == "-h" || arg == "--help");
            asked.then(|| subcommand_help(args)).flatten()
        }
    }
}

/// Render clap's help for the deepest built-in subcommand named in `args`, or `None` if none is present (top-level invocation, or a plugin name).
pub fn subcommand_help(args: &[String]) -> Option<String> {
    let mut root = Cli::command();
    root.build();

    let mut current = &root;
    let mut matched = false;
    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            continue;
        }
        match current.find_subcommand(arg) {
            Some(next) => {
                current = next;
                matched = true;
            }
            None => break,
        }
    }

    matched.then(|| {
        let mut cmd = current.clone();
        cmd.render_help().to_string()
    })
}

pub fn render_help(sym: &Symposium, cwd: &Path) -> String {
    let registry = load_registry(sym);
    let mut deps = sym.workspace_deps(cwd);
    let workspace = deps.crates();
    render(&registry, workspace)
}

fn render(registry: &PluginRegistry, workspace: &[WorkspaceCrate]) -> String {
    let mut cmd = Cli::command();
    let full = cmd.render_help().to_string();

    let (Some(commands_idx), Some(options_idx)) =
        (full.find("\nCommands:"), full.find("\nOptions"))
    else {
        return full;
    };

    let header = &full[..commands_idx];
    let options = &full[options_idx..];

    let deps = crate::crate_sources::crate_pairs(workspace);

    let humans = collect_section(&cmd, registry, &deps, Audience::Humans);
    let agents = collect_section(&cmd, registry, &deps, Audience::Agents);

    let col_width = humans
        .iter()
        .chain(agents.iter())
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0)
        + 2;

    let mut out = String::new();
    out.push_str(header);
    writeln!(out).unwrap();

    // Commands for Humans
    writeln!(out, "{HUMANS_HEADING}:").unwrap();
    for (name, desc) in &humans {
        writeln!(out, "{name:<col_width$}{desc}").unwrap();
    }
    writeln!(out).unwrap();

    // Commands for Agents
    writeln!(out, "{AGENTS_HEADING}:").unwrap();
    for (name, desc) in &agents {
        writeln!(out, "{name:<col_width$}{desc}").unwrap();
    }
    out.push_str(options);

    out
}

/// Collect entries for one audience section: clap's builtins first (sorted), then plugin-vended subs whose predicates apply (sorted).
fn collect_section(
    cmd: &Command,
    registry: &PluginRegistry,
    deps: &[(String, Version)],
    target: Audience,
) -> Vec<(String, String)> {
    let mut builtins = cmd
        .get_subcommands()
        .filter(|cmd| builtin_audience(cmd.get_name()) == Some(target))
        .map(|cmd| {
            (
                cmd.get_name().to_string(),
                cmd.get_about().map(|ss| ss.to_string()).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    builtins.sort();

    let mut plugins = applicable_subcommands(registry, deps)
        .into_iter()
        .filter(|(_, _, subcommand)| subcommand.audience == target)
        .map(|(_, name, subcommand)| (name.to_string(), subcommand.description.clone()))
        .collect::<Vec<_>>();

    plugins.sort();

    builtins.extend(plugins);

    builtins
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use expect_test::expect;

    use crate::{
        plugins::{ParsedPlugin, Plugin, Subcommand},
        predicate::PredicateSet,
    };

    use super::*;

    fn workspace_crate(name: &str, version: &str) -> WorkspaceCrate {
        WorkspaceCrate::new(name.into(), semver::Version::parse(version).unwrap(), None)
    }

    fn crate_set(spec: &str) -> PredicateSet {
        PredicateSet::from_crates(spec).unwrap()
    }

    fn plugin_with(
        name: &str,
        crates: &str,
        subcommands: BTreeMap<String, Subcommand>,
    ) -> ParsedPlugin {
        ParsedPlugin {
            path: PathBuf::from(format!("/test/{name}.toml")),
            plugin: Plugin {
                name: name.into(),
                hooks: vec![],
                predicates: crate_set(crates),
                skills: vec![],
                plugin_sources: vec![],
                mcp_servers: vec![],
                subcommands,
                installations: vec![],
                custom_predicates: vec![],
                discovery: Default::default(),
            },
            source_name: "test".into(),
            source_dir: PathBuf::from("/test"),
            source_provenance: std::collections::BTreeSet::new(),
        }
    }

    fn subcommand(description: &str, audience: Audience, crates: Option<&str>) -> Subcommand {
        Subcommand {
            description: description.into(),
            audience,
            command: "ignored".into(),
            predicates: crates.map(crate_set).unwrap_or_default(),
        }
    }

    fn registry(plugins: Vec<ParsedPlugin>) -> PluginRegistry {
        PluginRegistry {
            plugins,
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        }
    }

    /// Extract a named section's body(between heading and next blank line).
    fn extract_section<'a>(out: &'a str, heading: &str) -> &'a str {
        let start = out
            .find(heading)
            .unwrap_or_else(|| panic!("section `{heading}` not found in \n{out}"));

        let body_start = start + heading.len();
        let after = &out[body_start..];
        let end = after.find("\n\n").unwrap_or(after.len());

        &after[..end]
    }

    fn redact(s: String) -> String {
        s.replace(env!("CARGO_PKG_VERSION"), "$VERSION")
    }

    #[test]
    fn renders_with_no_plugin_subs() {
        let reg = registry(vec![]);
        let ws: Vec<WorkspaceCrate> = vec![];
        expect![[r#"
             AI the Rust Way

             Usage: cargo agents [OPTIONS] [COMMAND]

             Commands for humans:
             init         Set up user-wide configuration
             install      Install plugin sources into user config
             plugin       Manage plugins
             self-update  Update symposium to the latest version
             status       Show resolved plugin/skill state for the current workspace
             sync         Synchronize skills with workspace dependencies
             uninstall    Uninstall plugin sources from user config

             Commands for agents:
             crate-info   Find crate sources

             Options:
                   --update <UPDATE>  Control plugin source update behavior (none, check, fetch) [default: none] [possible values: none, check, fetch]
               -q, --quiet            Suppress status output
               -v, --verbose          Print detailed information about decisions made
                   --json             Output structured JSON report
               -h, --help             Print help
               -V, --version          Print version
         "#]]
             .assert_eq(&redact(render(&reg, &ws)));
    }

    #[test]
    fn humans_section_contains_humans_plugin_sub() {
        let mut subs = BTreeMap::new();
        subs.insert(
            "example-tool".to_string(),
            subcommand("Run the example tool", Audience::Humans, None),
        );
        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = vec![workspace_crate("example-crate", "1.0.0")];

        let out = render(&reg, &ws);
        let humans = extract_section(&out, "Commands for humans:");
        assert!(
            humans.contains("example-tool"),
            "human section missing example-tool:\n{humans}"
        );

        let agents = extract_section(&out, "Commands for agents:");
        assert!(
            !agents.contains("example-tool"),
            "example-tool leaked into agents section:\n{agents}"
        );
    }

    #[test]
    fn agents_section_contains_agents_plugin_sub() {
        let mut subs = BTreeMap::new();
        subs.insert(
            "example-tool".to_string(),
            subcommand("Analyze example-crate", Audience::Agents, None),
        );

        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = vec![workspace_crate("example-crate", "1.0.0")];

        let out = render(&reg, &ws);
        let agents = extract_section(&out, "Commands for agents:");
        assert!(
            agents.contains("example-tool"),
            "agents section missing example-tool:\n{agents}"
        );

        let humans = extract_section(&out, "Commands for humans:");
        assert!(
            !humans.contains("example-tool"),
            "example-tool leaked into humans section:\n{humans}"
        );
    }

    #[test]
    fn predicate_failure_hides_sub() {
        let mut subs = BTreeMap::new();
        subs.insert(
            "example-tool".to_string(),
            subcommand(
                "Analyze example-crate",
                Audience::Agents,
                Some("example-crate"),
            ),
        );

        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = vec![workspace_crate("other-crate-sources", "1.0.0")];

        let out = render(&reg, &ws);
        assert!(
            !out.contains("example-tool"),
            "example-tool should be filtered when workspace lacks example-crate:\n{out}"
        );
    }

    #[test]
    fn entries_sorted_within_section() {
        let mut subs = BTreeMap::new();
        subs.insert(
            "foo-tool".to_string(),
            subcommand("foo", Audience::Agents, None),
        );
        subs.insert(
            "bar-tool".to_string(),
            subcommand("bar", Audience::Agents, None),
        );

        let reg = registry(vec![plugin_with("example-plugin", "*", subs)]);
        let ws = vec![workspace_crate("example-crate", "1.0.0")];

        let out = render(&reg, &ws);
        let agents = extract_section(&out, "Commands for agents:");
        let bar_pos = agents.find("bar-tool").expect("bar-tool present");
        let foo_pos = agents.find("foo-tool").expect("foo-tool present");

        assert!(
            bar_pos < foo_pos,
            "bar-tool should precede foo-tool:\n{agents}"
        )
    }
}
