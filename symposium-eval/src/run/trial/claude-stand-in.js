#!/usr/bin/env node
// A stand-in for Claude Code in print mode, for tests that run a whole trial without a
// model. It speaks the `stream-json` events the adapter reads, runs the hooks that
// `~/.claude/settings.json` registers as Claude Code would, reads each installed skill,
// and leaves `double` in `src/lib.rs` unformatted, so only a formatter makes the
// formatting task pass.
'use strict';
const childProcess = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const VERSION = '2.1.316';
const args = process.argv.slice(2);
if (args[0] === '--version') {
    console.log(`${VERSION} (Claude Code)`);
    process.exit(0);
}
const option = (name) => args[args.indexOf(name) + 1];
const session = option('--session-id');
const model = option('--model');

const emit = (event) => process.stdout.write(`${JSON.stringify({ ...event, session_id: session })}\n`);

function registeredHooks(event) {
    const settings = path.join(process.env.HOME, '.claude', 'settings.json');
    if (!fs.existsSync(settings)) {
        return [];
    }
    const groups = JSON.parse(fs.readFileSync(settings, 'utf8')).hooks?.[event] ?? [];
    return groups.flatMap((group) => group.hooks.map((hook) => hook.command));
}

function runHooks(event, name, fields) {
    for (const command of registeredHooks(event)) {
        const hook = { hook_id: crypto.randomUUID(), hook_name: name, hook_event: event };
        emit({ type: 'system', subtype: 'hook_started', ...hook });
        const input = JSON.stringify({
            session_id: session,
            transcript_path: path.join(process.env.HOME, 'transcript.jsonl'),
            cwd: process.cwd(),
            hook_event_name: event,
            ...fields,
        });
        const ran = childProcess.spawnSync('sh', ['-c', command], { input, encoding: 'utf8' });
        emit({
            type: 'system',
            subtype: 'hook_response',
            ...hook,
            output: ran.stdout,
            stdout: ran.stdout,
            stderr: ran.stderr,
            exit_code: ran.status,
            outcome: ran.status === 0 ? 'success' : 'error',
        });
    }
}

runHooks('SessionStart', 'SessionStart:startup', { source: 'startup' });
emit({
    type: 'system',
    subtype: 'init',
    cwd: process.cwd(),
    tools: [],
    model,
    permissionMode: 'bypassPermissions',
    claude_code_version: VERSION,
    skills: [],
});
// Read each installed skill, as the agent does when the prompt names one.
const skills = path.join('.claude', 'skills');
for (const skill of fs.existsSync(skills) ? fs.readdirSync(skills) : []) {
    const id = `toolu_${crypto.randomUUID()}`;
    const file_path = path.join(skills, skill, 'SKILL.md');
    emit({
        type: 'assistant',
        message: { role: 'assistant', content: [{ type: 'tool_use', id, name: 'Read', input: { file_path } }] },
        parent_tool_use_id: null,
    });
    const content = fs.readFileSync(file_path, 'utf8');
    emit({
        type: 'user',
        message: { role: 'user', content: [{ tool_use_id: id, type: 'tool_result', content }] },
        parent_tool_use_id: null,
    });
}
fs.writeFileSync('src/lib.rs', '//! Small arithmetic helpers.\n\npub fn double(x:i32)->i32{x*2}\n');
runHooks('Stop', 'Stop', { stop_hook_active: false });
const tokens = { inputTokens: 1, outputTokens: 1, cacheReadInputTokens: 0, cacheCreationInputTokens: 0 };
emit({
    type: 'result',
    subtype: 'success',
    is_error: false,
    result: 'done',
    modelUsage: { [model]: tokens },
});
