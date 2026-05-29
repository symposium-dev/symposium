import type { Plugin } from "@opencode-ai/plugin";

// Symposium canonical input types (tagged enum).
type PreToolUseInput = {
  PreToolUse: {
    tool_name: string;
    tool_input: unknown;
    session_id: string | null;
    cwd: string | null;
  };
};

type PostToolUseInput = {
  PostToolUse: {
    tool_name: string;
    tool_input: unknown;
    tool_response: unknown;
    session_id: string | null;
    cwd: string | null;
  };
};

// Symposium canonical output types.
type PreToolUseOutput = {
  additionalContext?: string | null;
  updatedInput?: unknown | null;
};

type PostToolUseOutput = {
  additionalContext?: string | null;
};

type HookResult =
  | { ok: true; output: Record<string, unknown> }
  | { ok: false; denied: true; reason: string }
  | { ok: false; denied: false };

// Stashed information from pre-tool hooks that we surface in post-tool output.
type StashedInfo = {
  deniedMessage?: string;
  additionalContext?: string;
};

async function runHook(
  binary: string,
  event: string,
  input: unknown,
): Promise<HookResult> {
  const payload = JSON.stringify(input);
  try {
    const proc = Bun.spawn([binary, "hook", "opencode", event], {
      stdin: new Blob([payload]),
      stdout: "pipe",
      stderr: "pipe",
    });

    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ]);

    if (exitCode === 2) {
      return {
        ok: false,
        denied: true,
        reason: stderr.trim() || "hook denied execution",
      };
    }
    if (exitCode !== 0) {
      console.error(`[symposium] hook exited ${exitCode}: ${stderr.trim()}`);
      return { ok: false, denied: false };
    }
    const trimmed = stdout.trim();
    if (!trimmed) return { ok: false, denied: false };
    return { ok: true, output: JSON.parse(trimmed) };
  } catch (e) {
    console.error(`[symposium] failed to run hook:`, e);
    return { ok: false, denied: false };
  }
}

function findBinary(): string {
  return process.env.SYMPOSIUM_BINARY ?? "cargo-agents";
}

export const server: Plugin = async (ctx) => {
  const binary = findBinary();
  const cwd = ctx.directory;
  const stash = new Map<string, StashedInfo>();

  return {
    "tool.execute.before": async (input, output) => {
      const payload: PreToolUseInput = {
        PreToolUse: {
          tool_name: input.tool,
          tool_input: output.args,
          session_id: input.sessionID ?? null,
          cwd,
        },
      };

      const result = await runHook(binary, "pre-tool-use", payload);

      if (!result.ok && result.denied) {
        stash.set(input.callID, {
          deniedMessage: result.reason,
        });
        return;
      }

      if (!result.ok) return;

      const hookOutput = result.output as PreToolUseOutput;
      const info: StashedInfo = {};

      if (hookOutput.updatedInput != null) {
        output.args = hookOutput.updatedInput;
      }

      if (hookOutput.additionalContext) {
        info.additionalContext = hookOutput.additionalContext;
      }

      if (info.additionalContext) {
        stash.set(input.callID, info);
      }
    },

    "tool.execute.after": async (input, output) => {
      const payload: PostToolUseInput = {
        PostToolUse: {
          tool_name: input.tool,
          tool_input: input.args,
          tool_response: output.output,
          session_id: input.sessionID ?? null,
          cwd,
        },
      };

      const result = await runHook(binary, "post-tool-use", payload);

      // Collect all context to append.
      const parts: string[] = [];

      // Drain stashed info from the pre-tool phase.
      const info = stash.get(input.callID);
      if (info) {
        stash.delete(input.callID);
        if (info.deniedMessage) {
          parts.push(
            `[symposium] Warning: a hook attempted to block this tool call but OpenCode does not support blocking. Reason: ${info.deniedMessage}`,
          );
        }
        if (info.additionalContext) {
          parts.push(info.additionalContext);
        }
      }

      // Add post-tool context.
      if (result.ok) {
        const hookOutput = result.output as PostToolUseOutput;
        if (hookOutput.additionalContext) {
          parts.push(hookOutput.additionalContext);
        }
      }

      if (parts.length > 0) {
        const suffix = "\n\n" + parts.join("\n\n");
        output.output = (output.output ?? "") + suffix;
      }
    },
  };
};
