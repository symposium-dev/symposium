# RFD Process

## TL;DR

- Add a lightweight RFD (Request for Discussion) process for planning larger changes.
- RFD PRs focus on the design and plans.
- For trusted contributors, once an RFD is accepted, they are encouraged to land PRs independently.

## Motivation

It is easier to understand changes by talking first through the intended design and in particular the user-facing impact. Implementation is often secondary, particularly in an era of agentic development. The RFD process is designed to focus our attention on the design and plans.

This particular RFD process is also experimenting with the development process to account for the use of agents. In an agentic environment, the review process often becomes the bottleneck, so we want to focus our discussion on the design and plans. Coding can then proceed more easily. The RFD also serves as a reference for agents doing a review.

## Change in a nutshell

When planning a larger change, first create a PR adding an RFD that lays out the design and plans. Once the PR is accepted, implementation PRs should modify the implementation plan and keep it up-to-date, making it easier for reviewers or others to understand how the work that is being done relates to the design.

Once the RFD is accepted, team members are encouraged to land PRs independently if they feel confident in the changes. For newer contributors, review should be a simpler process.

Each RFD is created as a subdirectory under `md/rfds` followed a [standard template](../TEMPLATE/README.md). It can consist of a single file, but you are also encouraged to leverage the subdirectory structure to include other files such as sample documentation, images, or other planning documents.

## Detailed plans

### Template structure

The template contains the following sections:

- **TL;DR** — bullet points covering the key changes
- **Motivation** — why we're making this change
- **Change in a nutshell** — the most important changes
- **Detailed plans** — full design, with subchapters as needed
- **Frequently asked questions** — rationale, alternatives, discussion
- **Implementation plan and status** — checklist of steps, updated as work lands

### Style requirements

- No promotional text or dramatic language. Be factual and brief.
- Lead with concrete concepts, then generalize.
- Include examples (code snippets, config fragments).
- Include proposed user-facing documentation as subchapters when the change affects docs.

### Agent skill

The `.agents/skills/authoring-rfds/SKILL.md` skill teaches agents the process and style guide so they can help draft RFDs consistently.

We may wish to create additional skills over time similar to [github spec-kit](https://github.com/github/spec-kit), e.g., for impl planning, review, etc.

### Copilot code review instructions

The `.github/copilot-code-review-instructions.md` file configures GitHub's Copilot reviewer to:

- Check whether a PR relates to an active RFD and, if so, verify that the implementation plan is updated, the approach is consistent with the design, and any deviations are documented.
- Watch for Rust coding practices: exhaustive matches over wildcards, narrow visibility defaults, no backwards-compatibility stubs, and documentation that describes current behavior rather than historical context.

### Decision making

Input from multiple core team members is preferred, but any single core team member can accept or reject an RFD independently. The BDFL has final call if there's disagreement.

## Frequently asked questions

### Are RFDs required for every change?

No. They're for larger changes where upfront discussion helps — roughly, anything that touches multiple modules or introduces new concepts. Bug fixes, small features, and refactors don't need one.

### Why is this process so lightweight?

The goal is to stay out of the way and keep a record. Merging an RFD or even a PR doesn't commit us to anything — issuing a release does. That's the point where we need to be careful, not at the proposal or implementation stage.

### What if the design changes during implementation?

Update the RFD's status section to note deviations. The RFD is a living document until it moves to "Completed".

## Implementation plan and status

### Initial PR

- [x] Add RFD template (`md/rfds/TEMPLATE/README.md`)
- [x] Add RFD index pages (`accepted.md`, `completed.md`)
- [x] Add SUMMARY.md entries
- [x] Add authoring skill (`.agents/skills/authoring-rfds/`)
- [x] Write this meta-RFD as a demonstration

No tests required.
