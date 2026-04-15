# Hooks

Many (but not all) agents define the concept of *hooks*, which are callbacks that can be executed by the agent when certain events occur. For example, most agents offer a way to get a callback each time the user submits a prompt, or each time a tool is invoked.

## We want to allow people to write hooks that work across agents

We wish to allow plugins to react to hook events and we want those plugins to work uniformly across all agents. We also want to be able to include existing systems as easily as possible -- so for example if a system like rtk expects claude code events, we want to be able to use it, even if the user is using Gemini or Codex.

This is a challenge because there is no "hook standard" and so while the concept is common the details vary per agent. The set of hooks are different. The way hooks are invoked is different: most agents invoke an external command and give it JSON, but e.g. Opencode uses Javascript plugins. Even for those agents that supply JSON, the precise fields that are present or missing are different, and the range of responses allowed by the hook are different.

## We support all the formats and define a common format

Our approach is to support as many formats as we can, both for input and output. We also define a common format, symposium hooks, and we have the ability to convert from each agent's format into symposium hooks and vice versa as faithfully as we can.

## Agents tell us what format message they are providing

We register symposium as a "hook command" for whatever agents the user is using. Most commonly, the agent will invoke the symposium CLI tool with an argument that says what agent this is and what hook they are invoking, e.g., `symposium hook claude pre-tool-use`. We use this information to parse the incoming json into an agent-specific format (e.g., `claude::PreToolUse`) and then to convert *that* into the standard symposium format (e.g., `symposium::PreToolUse`).

Once the hook processing is done, the result will be a symposium output object, derived via the mechanisms described below. This is then convert back into the agent's native format.

## Builtin hooks operate on symposium

Our builtin hooks operate on symposium events and thus can receive events from any agent. They then return the symposium response structs back out. These are converted into the format that the agent expects. 

## Plugins tell us what format they expect

When a plugin is added into symposium that wishes to receive hook events, it specifies what format it expects. This can be either a specific agent format (e.g., `claude`) or the symposium format. We will invoke those hooks by

* if the format we received is the format the plugin expects, we provide the original input from the agent;
* otherwise, we convert our symposium format into the format they expect. This may be mildly lossy so we prefer not to do it.

The agent then gives us output in the same format. If this output is the same as what the agent is expecting, we keep the plugin's output as is. Otherwise, we convert the plugin output into symposium format and then convert from there into the agent's output.

## Combining outputs

When multiple hooks are defined, their outputs are merged by doing JSON recursive field-by-field merging, e.g.,

```
{ "a": 1 } + { "b": 2 } = { "a": 1, "b": 2 }
{ "a": {"x": 3} } + { "a": {"y": 4} } = { "a": { "x": 3, "y": 4 } }
```
