# Common issues

## Known hook implementation gaps

The following issues were identified by auditing our hook implementations against the agent reference docs (`md/design/agent-details/`). They don't cause crashes (the fallback path handles events without agent-specific handlers) but mean some features are incomplete.

### `toolArgs` not parsed (Copilot)

Copilot sends `toolArgs` as a JSON *string* (not an object). Our `CopilotPreToolUsePayload` declares it as `serde_json::Value` and passes it through as-is in `to_hook_payload()`. Downstream code expecting structured tool args will get a raw string. Should parse the JSON string into a `Value` during conversion.

### `permissionDecision` dropped (Copilot)

`CopilotPreToolUseOutput::from_hook_output()` never maps `permissionDecision` or `permissionDecisionReason` from the builtin hook output. If a builtin handler wants to deny a tool call, the decision is silently lost in Copilot output.

## Antigravity footguns

Two Antigravity behaviours fail silently rather than loudly, so they are worth
knowing before debugging a hook that "does nothing".

### `{}` on `PreToolUse` denies the tool call

Antigravity ignores hook exit codes entirely; only stdout decides. On
`PreToolUse`, an object without a valid `decision` — `{}` included, as well as
`{"decision": ""}` — is treated as a **denial**, while writing nothing at all
allows. Symposium's dispatcher returns `{}` whenever no plugin contributed, which
is the common case, so `AntigravityPreToolUseOutput` keeps `decision` as a plain
always-serialized field defaulting to `allow`. Making it `Option` or adding
`skip_serializing_if` would block every tool call.

### Unknown event names and shapes are accepted and never fire

An unrecognised event key in `hooks.json`, or the wrong structure for a known one
(a flat handler list where a `matcher` group is expected, or vice versa), produces
no error — the hook simply never runs. `ANTIGRAVITY_EVENTS` and
`antigravity_is_tool_event` in `agents/mod.rs` are the single source of truth for
both, and the unit tests assert the shape per event for exactly this reason.

## Windows portability (tests)

The test suite runs on `windows-latest`. A few patterns recur when writing tests that touch paths or scripts:

- **Paths in TOML/JSON string literals.** A Windows path like `C:\Users\...` is invalid inside a TOML or JSON string (the backslashes read as escapes). When substituting a real path into fixture text, convert to forward slashes first; Windows accepts `/` in paths. See `setup_fixture` in `symposium-testlib`.
- **Paths inside `sh` script bodies.** On Windows `sh` is git-bash's MSYS shell, which reads `C:\a\b` as escapes plus an illegal `:`. Rewrite to the `/c/a/b` form and quote the value. See `sh_path` in `predicate.rs` tests.
- **`.sh` files must use `script`, not `executable`.** A shell script cannot be spawned directly as a process on Windows (no shebang support). In fixtures, reference it via `script = "..."` so it is run through `sh`, never `executable = "..."`.
- **Canonicalized paths carry a `\\?\` prefix.** `fs::canonicalize` on Windows returns an extended-length path that `cargo`'s output lacks. Canonicalize both sides before comparing.
- **Snapshot tests and home-abbreviated paths.** `display_path` (in `output.rs`) abbreviates `$HOME` to `~/`. On Windows the test temp dir lives under `$HOME`, so printed config paths come out home-relative, not absolute. `normalize_paths` (in `symposium-testlib`) replaces both the absolute and the `~/` form; a snapshot leaking a random `.tmpXXXX/` path means one form was missed. Do not `UPDATE_EXPECT` your way past it: that bakes the volatile temp path into the snapshot and it fails on the next run.
