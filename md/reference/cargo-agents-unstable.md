# Unstable agent commands

The commands in this section are invoked by AI agents, not by users directly. They are hidden from `cargo agents --help`, and their arguments, output format, and exit codes may change in future releases without notice.

Currently this is `cargo agents hook`, the hook protocol entry point. (`crate-info` is also agent-facing but is no longer hidden — it appears under "Commands for agents" in `cargo agents --help`.)
