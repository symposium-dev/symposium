# Tickets and the project board

Work is tracked as GitHub issues on the [Symposium project board](https://github.com/orgs/symposium-dev/projects/6). This page covers how the board is organised and how to write a ticket that someone (human or agent) can pick up and build.

The goal is that a ticket's description is reviewed before the work starts. A good description is quick to review and leaves the implementer few open questions.

## The board

Each item has one of these statuses:

| Status | Meaning |
| --- | --- |
| Backlog | Recorded, not yet planned |
| Discussion | The what or the how is still being discussed (on Zulip, in the sync meeting, or in an RFD) |
| Todo | Description is reviewed and the work is ready to pick up |
| In Progress | Someone is working on it |
| Review | A PR is open |
| Done | Merged or otherwise closed |

## Issue types

- **Feature**: an epic or initiative, made of several pieces of work. For example #393 (Agent plugins) or #394 (Automated plugin audits).
- **Task**: one ticket-sized piece of work, usually one PR. For example #399.
- **Bug**: something Symposium does wrong. For example #380.

The type carries the size, so don't add "Size: epic" or similar to the description.

## Ticket, RFD, or epic?

These are guidelines, not rules. The person filing the issue makes the call, and a reviewer may suggest a different one.

- If the design is settled and only the work remains, it is usually a **Task**.
- If the change is larger and the design or its user-facing impact is still contested, consider an [RFD](../rfds/rfd-process/README.md) first.
- If it is a whole initiative, it is usually a **Feature**, broken down into RFDs and Tasks as the design firms up.
- If Symposium does something wrong, it is a **Bug**.

An agent drafting a ticket suggests a type, and whether an RFD might be needed, and asks the person it works for to confirm.

## Writing a ticket

### Titles

Say what changes, in plain words. No scenario numbers or `[tag]` prefixes.

- Bad: `[S3] notice`
- Good: `Show the plugin consent notice once per session`

RFD PRs are titled `RFD: <title>`, for example `RFD: Automated plugin audits`.

### Requirements and examples

Describe what done means with requirements and examples. The vocabulary comes from [Habeas Codicem](https://nikomatsakis.github.io/habeas-codicem/); we use the vocabulary only, not its evidence files or tooling.

- A **requirement** is one MUST or MUST NOT statement with a short name.
- An **example** is a concrete case: a situation, an action, and the expected result. It cites the requirement it illustrates.

```markdown
**Requirements**
- `notice.once`: Symposium MUST show the consent notice at most once per agent session.
- `notice.no-block`: The notice MUST NOT wait for user input.

**Examples**
- `notice.once`: a workspace has two dependencies awaiting consent; the user starts a
  session and sends three prompts; the notice appears once, at session start, naming both.
```

If an RFD will refine the requirements, mark them provisional.

### Task layout

See #399 for a full example.

- **Roadmap**: a link to the roadmap the ticket came from (currently [this one](https://hedgedoc.symposium.dev/M--v_Y0HRquBsuJiX1nUlw); older tickets point at the [previous roadmap](https://hedgedoc.symposium.dev/9lgiRxjHQDGPAITrYky8Lg)).
- **What and why**: one or two sentences a user would understand.
- **Requirements**
- **Examples**
- **Out of scope**: what a reader might expect here but is deliberately left out.
- **Open questions**

### Feature layout

Open Features with the **Initiative** issue form (`.github/ISSUE_TEMPLATE/initiative.yml`), which sets the type and adds the issue to the board. See #407 for a full example. The form's sections:

- **What and why**
- **Definition**: requirements and examples, as above.
- **Zulip discussion**: where the initiative is discussed.
- **RFD**: the RFD PR, if there is one.
- **Code PRs**: a checklist filled in as PRs open. Each PR says "Part of #N" in its description.
- **Notes**: the roadmap it came from, dependencies on other initiatives, and open questions.

### Bug layout

Open bugs with the **Bug** issue form (`.github/ISSUE_TEMPLATE/bug.yml`), which sets the type and the `bug` label. See #380 or #369 for full examples. The form's sections:

- **What happens**: one or two sentences.
- **Steps to reproduce**: numbered, with the config or manifest involved.
- **Actual result** and **Expected result**: exact output or error text for the actual one.
- **Environment**: Symposium version or commit, agent and its version, OS.
- **Notes**: suspected cause, possible fixes, related issues or PRs.

A bug needs no description review before it is fixed, unless the expected result is itself a design question. Then treat it like a Task.

## From ticket to PR

1. Draft the description and move the item to Discussion.
2. Get a quick review of the description from a core team member (on [Zulip](https://symposium-dev.zulipchat.com) or in the [sync meeting](./governance.md#sync-meeting)), then move it to Todo.
3. Build it. The PR follows the PR template and the [disclosure policy](./governance.md#pr-disclosure-policy), and says "Part of #N" or "Closes #N".

Review requirements for the PR itself are in the [merge policy](./governance.md#pr-merge-policy).
