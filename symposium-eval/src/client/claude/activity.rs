//! The file mutations and shell commands a Claude Code transcript shows the agent asking
//! for, read from the call record after the pass.
//!
//! `Write` and `Edit` name their file in `file_path`, `NotebookEdit` in `notebook_path`;
//! `Bash`, `PowerShell` and `Monitor` run a `command`. The recorded `tools_used`
//! transcript pins all but `PowerShell` and `Monitor`, which follow Claude Code's
//! documented inputs. Other tools are not read.

use std::path::Path;

use serde::Deserialize;

use super::calls::{Calls, Scope, ToolCall};
use crate::client::{CappedText, FileMutation, ObservedPath, ShellCommand, ToolActivity, paths};

/// More of a command than a reader of results needs; the rest is only counted.
const MAX_COMMAND: usize = 4 * 1024;

#[derive(Deserialize)]
struct FileInput {
    file_path: String,
}

#[derive(Deserialize)]
struct NotebookInput {
    notebook_path: String,
}

#[derive(Deserialize)]
struct CommandInput {
    command: String,
}

/// What a call would be, by its tool.
enum Kind {
    Mutation(ObservedPath),
    Command(String),
    /// A call of a tool that runs commands, without one that can be read.
    UnreadableCommand,
    Other,
}

pub(super) fn activity(calls: &Calls) -> ToolActivity {
    let mut activity = ToolActivity {
        mutations: Vec::new(),
        commands: Vec::new(),
        complete: calls.complete(),
    };
    for call in calls.iter() {
        let kind = kind(call);
        if call.scope == Scope::Unscoped {
            // A call no session vouches for could be one this session hid.
            if !matches!(kind, Kind::Other) {
                activity.complete = false;
            }
            continue;
        }
        match kind {
            Kind::Mutation(path) => activity.mutations.push(FileMutation {
                path,
                result: call.result,
            }),
            Kind::Command(command) => activity.commands.push(ShellCommand {
                text: CappedText::new(&command, MAX_COMMAND),
                result: call.result,
            }),
            Kind::UnreadableCommand => activity.complete = false,
            Kind::Other => {}
        }
    }
    activity
}

fn kind(call: &ToolCall) -> Kind {
    let input = &*call.input;
    let path = |reported: Option<String>| match reported {
        Some(reported) => {
            let resolved = paths::lexical_absolute(&call.cwd, Path::new(&reported));
            ObservedPath::Readable { reported, resolved }
        }
        None => ObservedPath::Unreadable,
    };
    match call.name.as_str() {
        "Write" | "Edit" => Kind::Mutation(path(
            FileInput::deserialize(input)
                .ok()
                .map(|input| input.file_path),
        )),
        "NotebookEdit" => Kind::Mutation(path(
            NotebookInput::deserialize(input)
                .ok()
                .map(|input| input.notebook_path),
        )),
        "Bash" | "PowerShell" | "Monitor" => match CommandInput::deserialize(input) {
            Ok(input) => Kind::Command(input.command),
            Err(_) => Kind::UnreadableCommand,
        },
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::client::ToolResult;
    use crate::client::claude::fixtures::{
        TOOLS_USED, before, edited, from, line_with, recorded_session, replaced, without,
    };
    use crate::client::claude::transcript::read_text;
    use crate::client::claude::version;

    fn activity_of(transcript: &str) -> ToolActivity {
        read_text(
            transcript,
            Path::new("/trial/project"),
            recorded_session(),
            version::MINIMUM,
            &[],
        )
        .activity
    }

    fn mutation(reported: &str, result: ToolResult) -> FileMutation {
        FileMutation {
            path: ObservedPath::Readable {
                reported: reported.to_owned(),
                resolved: Some(PathBuf::from(reported)),
            },
            result,
        }
    }

    const BASH: &str = r#""name":"Bash","input":{"command":"echo three > other.txt""#;

    #[test]
    fn the_recorded_mutations_and_command_are_facts() {
        let activity = activity_of(TOOLS_USED);

        assert_eq!(
            activity.mutations,
            [
                mutation("/trial/project/notes.txt", ToolResult::Succeeded),
                mutation("/trial/project/notes.txt", ToolResult::Succeeded),
                // Claude Code refused it: the notebook had not been read first.
                mutation("/trial/project/nb.ipynb", ToolResult::Failed),
            ]
        );
        assert_eq!(
            activity.commands,
            [ShellCommand {
                text: CappedText::new("echo three > other.txt", MAX_COMMAND),
                result: ToolResult::Succeeded,
            }]
        );
        assert!(activity.complete);
    }

    #[test]
    fn a_relative_path_resolves_where_the_session_ran() {
        let transcript = replaced(
            TOOLS_USED,
            r#""notebook_path":"/trial/project/nb.ipynb""#,
            r#""notebook_path":"../evidence/nb.ipynb""#,
        );

        let activity = activity_of(&transcript);

        assert_eq!(
            activity.mutations[2].path,
            ObservedPath::Readable {
                reported: "../evidence/nb.ipynb".to_owned(),
                resolved: Some(PathBuf::from("/trial/evidence/nb.ipynb")),
            }
        );
    }

    #[test]
    fn a_mutation_without_a_readable_path_is_kept_as_unreadable() {
        let transcript = replaced(
            TOOLS_USED,
            r#""notebook_path":"/trial/project/nb.ipynb""#,
            r#""notebook_path":7"#,
        );

        let activity = activity_of(&transcript);

        assert_eq!(activity.mutations[2].path, ObservedPath::Unreadable);
        assert!(activity.complete);
    }

    #[test]
    fn a_command_that_cannot_be_read_leaves_the_record_incomplete() {
        // `Monitor` too: no recording shows a shape of it that runs nothing.
        for tool in ["Bash", "PowerShell", "Monitor"] {
            let call = format!(r#""name":"{tool}","input":{{"cmd":"x""#);

            let activity = activity_of(&replaced(TOOLS_USED, BASH, &call));

            assert!(activity.commands.is_empty(), "{tool}");
            assert!(!activity.complete, "{tool}");
        }
    }

    #[test]
    fn powershell_and_monitor_run_commands() {
        let as_tool = |tool: &str| {
            let call = format!(r#""name":"{tool}","input":{{"command":"echo three > other.txt""#);
            activity_of(&replaced(TOOLS_USED, BASH, &call))
                .commands
                .len()
        };

        assert_eq!(as_tool("PowerShell"), 1);
        assert_eq!(as_tool("Monitor"), 1);
    }

    #[test]
    fn a_command_is_kept_only_to_its_limit() {
        let text = format!("echo {}é", "x".repeat(MAX_COMMAND - 6));

        let capped = CappedText::new(&text, MAX_COMMAND);

        // The `é` straddles the limit, so the prefix stops before it.
        assert_eq!(capped.prefix.len(), MAX_COMMAND - 1);
        assert_eq!(capped.original_bytes, text.len());
    }

    #[test]
    fn a_session_without_its_ending_cannot_clear_anything() {
        assert!(!activity_of(&without(TOOLS_USED, "result")).complete);
    }

    #[test]
    fn a_mutation_no_session_vouches_for_leaves_the_record_incomplete() {
        let write = line_with(TOOLS_USED, r#""name":"Write""#);
        // A user message may name no session; a call in one counts for nothing.
        let unscoped = edited(write, |event| {
            event.insert("type".to_owned(), "user".into());
            event.remove("session_id").unwrap();
        });
        let transcript = format!(
            "{}{unscoped}\n{}",
            before(TOOLS_USED, "result"),
            from(TOOLS_USED, "result")
        );

        let activity = activity_of(&transcript);

        assert_eq!(activity.mutations.len(), 3);
        assert!(!activity.complete);
    }
}
