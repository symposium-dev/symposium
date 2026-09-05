<!-- See .agents/skills/authoring-rfds/SKILL.md for style guidance -->

# Replace Gemini CLI with Antigravity CLI

## TL;DR

- Retire the `gemini` agent and add `antigravity` (the `agy` CLI) in its place.
- Skills reuse the vendor-neutral `.agents/skills/` path symposium already writes; global skills go to `~/.gemini/config/skills/`.
- MCP servers move to a dedicated `mcp_config.json`, keeping the familiar `mcpServers` shape.
- Hooks are a new wire format on a new file, and follow the configured `hook-scope` like any other agent.
- `SessionStart` is undocumented but real, and fires once per session. `PreInvocation` stands in for `user-prompt-submit`, gated on the first invocation of a turn.
- A `PreToolUse` hook that returns `{}` **denies the tool call**. Symposium must emit an explicit allow.

## Motivation

Gemini CLI can no longer sign in. Choosing "Sign in with Google" now fails with a server-side message from Google:

> Failed to sign in. Message: This client is no longer supported for Gemini Code Assist for individuals. To continue using Gemini, please migrate to the Antigravity suite of products: https://antigravity.google

The `google-gemini/gemini-cli` repository is not archived and carries no deprecation notice, and the API-key and Vertex AI paths may still authenticate. But the consumer sign-in that symposium's users rely on is closed, and Google's own message names Antigravity as the destination. Supporting an agent most users cannot log into is not worth a second hook wire format.

Antigravity is also where the extensibility work is going: `agy plugin import gemini` migrates Gemini extensions, skills and settings, and its config lives under the same `~/.gemini/` root.

Antigravity is not a rename. Every axis symposium depends on changed: hook file, hook shape, event names, payload field names, output contract, and MCP location. Keeping both agents would mean carrying a second hook wire format for a surface we do not expect users to stay on, so this replaces rather than adds.

## Change in a nutshell

Skills are the part that already works. Antigravity reads the same layout symposium writes today for Copilot, Codex, OpenCode and Goose:

```
.agents/skills/<name>/SKILL.md
```

Extra files in a skill directory are explicitly supported, so symposium's `.symposium` marker and wildcard `.gitignore` survive, and marker-based stale reaping keeps working unchanged.

Hooks are the part that is genuinely new. Registrations are keyed by a **hook name** and live in their own file, not in `settings.json`:

```json
{
  "symposium": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [{ "type": "command", "command": "cargo-agents hook antigravity pre-tool-use" }]
      }
    ],
    "PreInvocation": [
      { "type": "command", "command": "cargo-agents hook antigravity pre-invocation" }
    ]
  }
}
```

Note the two shapes: `PreToolUse`/`PostToolUse` wrap handlers in a `matcher` group, while `PreInvocation`/`PostInvocation`/`Stop` are flat handler lists.

## Detailed plans

### Paths

| Axis | Project | Global |
|---|---|---|
| Hooks | `.agents/hooks.json` | `~/.gemini/config/hooks.json` |
| Skills | `.agents/skills/<name>/SKILL.md` | `~/.gemini/config/skills/<name>/` |
| MCP | `.agents/mcp_config.json` | `~/.gemini/config/mcp_config.json` |

`~/.gemini/config/` is the current global root. `~/.gemini/antigravity-cli/` holds runtime state and `settings.json`; earlier documentation points at it for plugins and skills, but a `.migrated` marker shows that move already happened.

### Scope behaves normally

Both scopes work. `agy` discovers `.agents/` by walking up from the working
directory, so project-scoped hooks and skills load without any special launch
flag, and symposium can honor the configured `hook-scope` as it does for every
other agent.

One caveat is worth recording because it is easy to misdiagnose. In headless
print mode (`agy -p`), `agy` adopts no workspace at all unless it is given
`--add-dir <absolute path>`: it cannot read project files, and it loads no
project `.agents/` configuration. The interactive TUI adopts the working
directory on its own and loads project hooks on a second pass after adoption.
So a hook that appears not to fire under `agy -p` is not necessarily
misregistered — a relative `--add-dir .` does not work either, it must be
absolute. This affects automation and CI, not ordinary interactive use.

### Events

| Symposium event | Antigravity event |
|---|---|
| `pre-tool-use` | `PreToolUse` |
| `post-tool-use` | `PostToolUse` |
| `session-start` | `SessionStart` |
| `stop` | `Stop` |
| `user-prompt-submit` | `PreInvocation`, first invocation of a turn |

Antigravity's documentation lists five events and omits `SessionStart`, but the binary's hook proto carries six, and a `SessionStart` key in `hooks.json` loads, fires **once per session** (verified across a `--continue` turn) and carries a populated `workspacePaths`. So session start maps natively and needs no derivation.

`user-prompt-submit` is the one approximation. Antigravity has no prompt event, and `PreInvocation` fires before *every* model call — several times in a turn that uses tools — so dispatch runs the prompt event only when `invocationNum == 0`. That is stateless; no session tracking is involved. An unknown event key, incidentally, is accepted silently and never fires, so a typo there fails quietly.

### The allow contract

`PreToolUse` output decides the call, and exit codes are ignored entirely. Writing nothing allows. Writing `{}` **denies** — as do `{"decision": ""}` and any object carrying only other fields. Symposium's dispatcher returns `{}` when no plugin contributed, which is the common case, so the Antigravity output conversion must emit `{"decision": "allow"}` unless a plugin actually denied.

`PreInvocation` carries context back the way `additionalContext` does for Claude:

```json
{ "injectSteps": [{ "ephemeralMessage": "..." }] }
```

This is how the session-start discovery hint, consent hint and update nudge reach the model.

### Tool names

Antigravity tool names are the lowercased step type without its `CORTEX_STEP_TYPE_` prefix — `run_command`, `view_file`, `browser_*`. A plugin hook matcher written for Claude (`Bash`, `Edit`) will not match. This affects plugin authors, not symposium's own dispatch, and belongs in the hook reference.

### Retiring Gemini

Removal reuses the retired-agent machinery: a retired name in the user config is reported and skipped, a stale `cargo-agents hook gemini` registration exits cleanly, a plugin manifest declaring `format = "gemini"` still loads with that hook skipped, and a one-shot migration clears the leftovers under `~/.gemini/` that nothing would otherwise reap.

See [proposed agent details](./proposed-agent-details.md) for the reference page this produces.

## Frequently asked questions

### Why not support both Gemini and Antigravity?

Antigravity shares no hook wire format with Gemini — different file, shape, event names, payload fields and output contract. Supporting both means two full hook schemas for two surfaces with one user base. Google ships a migration path from one to the other, so users are not expected to hold both.

### Why doesn't anything fire under `agy -p`?

Headless print mode adopts no workspace unless given `--add-dir <absolute path>`.
Without it `agy` cannot read project files at all, so no project `.agents/`
configuration loads either. The interactive TUI adopts the working directory by
itself. Any automation driving `agy -p` against a project must pass the absolute
path; a relative `.` is ignored.

### Should the headless behaviour be reported upstream?

Worth reporting, but nothing here depends on it. Headless mode silently having
no workspace — rather than refusing, or defaulting to the working directory — is
surprising enough to be worth a bug, but symposium's registration is correct
either way and no part of this design works around it.

### Does this affect the agent-plugin work?

Antigravity's plugin manifest is `plugin.json` at a plugin root — the same filename as the Agent Plugins standard, with a different schema — so the two cannot share one compiled directory. `agy plugin import claude` exists and may cover the case without a dedicated emitter. That is left to the agent-plugin RFD; this one does not add a plugin emitter.

## Implementation plan and status

Two deviations from the plan above, recorded as implementation proceeded:

- **Step 5 changed.** It proposed deriving a session start by tracking seen
  `conversationId`s. Testing showed `SessionStart` exists natively and fires once
  per session, so the derivation was dropped and the step became the
  `user-prompt-submit` gate instead. The events section above reflects the native
  event.
- **Order changed.** Antigravity is added first and Gemini retired last, so every
  commit leaves users with a working agent. Step 1 therefore lands after steps 2
  to 6.

One fix fell out of the work and is not listed as a step: `sync` registered hooks
and MCP by passing the workspace root to the *global* functions, which only
produces the right path for agents whose project and global locations share a
shape. Antigravity's do not, so project scope wrote to a file `agy` never reads.
The project-scoped functions existed but had no callers; `sync` now dispatches to
them, which also moves Copilot's project hooks to `.github/hooks/`.

### Step 1: Retire the `gemini` agent

Remove the agent, its hook schema and its MCP registration, keeping the retired-name shims and the one-shot cleanup migration. Verify with the existing init/sync suite plus tests that a stale `gemini` config entry is warned about rather than fatal, and that `cargo agents hook gemini <event>` exits zero.

- [ ] not started

### Step 2: Add the `antigravity` agent for skills and MCP

Agent enum, skill paths, and `mcp_config.json` registration. No hooks yet, so the agent is skills-only at this point. Verify by syncing a workspace and asserting the skill directory and MCP entry land at the paths above; confirm against a real `agy` that the MCP server is listed.

- [ ] not started

### Step 3: Hook registration and unregistration

Write and reap symposium's named entry in `hooks.json` at the configured scope. Unregistration keys on the hook name symposium owns, so unrelated entries in a shared file survive. Verify that registering, re-registering and removing the agent leaves other named hooks intact, and confirm against a real `agy` that the hook loads at both project and global scope.

- [ ] not started

### Step 4: Hook wire format

Input and output conversion for the five events, including the explicit allow on `PreToolUse` and `injectSteps` for context. Verify with round-trip tests per event, a test asserting a no-contribution dispatch emits an allow rather than `{}`, and an end-to-end check that a tool call is not denied.

- [ ] not started

### Step 5: Gate `user-prompt-submit`

`PreInvocation` fires before every model call, so dispatch must run the prompt event only on `invocationNum == 0` or plugin prompt hooks fire several times per turn. Verify that a turn making a tool call (two invocations) dispatches `user-prompt-submit` exactly once, and that `session-start` fires once per session across two turns.

- [ ] not started

### Step 6: Documentation

Agent details page, supported-agents entry, and the `--add-dir` caveat where users will meet it.

- [ ] not started
