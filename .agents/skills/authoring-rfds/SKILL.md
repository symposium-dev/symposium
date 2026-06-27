---
name: authoring-rfds
description: Write and structure RFDs (Requests for Discussion) for the Symposium project. Use when creating a new RFD, drafting design proposals, or reviewing RFD content for style compliance.
---

# Authoring RFDs

## What is an RFD?

An RFD is a proposal for a non-trivial change. It lives in `md/rfds/<name>/README.md` and follows the template in `md/rfds/TEMPLATE/README.md`. RFDs are submitted as PRs; once merged they appear under the "Accepted" section of `md/SUMMARY.md`. When implementation is complete, they move to "Completed".

## Process

1. Create a subdirectory under `md/rfds/` (e.g., `md/rfds/my-feature/`).
2. Copy the template from `md/rfds/TEMPLATE/README.md` into `md/rfds/my-feature/README.md`.
3. Fill in each section.
4. Add the RFD to `md/SUMMARY.md` under the "Accepted" heading.
5. Open a PR. Discussion happens on the PR.
6. Implementation PRs reference the RFD and update its status section.

## Style guide

### Voice and tone

- No promotional text, overstated claims, or dramatic writing style.
- Be factual, brief, and to the point.
- Write as if explaining to a colleague who is already familiar with the project.
- Avoid filler phrases like "This exciting new feature will revolutionize..." — just say what it does.

### Structure

- Lead with concrete concepts, then generalize. Show the specific thing first, explain the pattern second.
- Include examples. A short code snippet or config fragment is worth more than a paragraph of description.
- Keep sections short. If a section exceeds ~30 lines, consider splitting it or using sub-headings.

### Proposed documentation

- Include subchapters with proposed user-facing documentation (e.g., what the relevant reference page or guide section would look like after the change lands).
- This serves two purposes: it forces concrete thinking about how the feature is presented, and it gives reviewers something tangible to react to.
- Place these in additional files within the RFD directory (e.g., `md/rfds/my-feature/proposed-reference.md`).

### Frequently asked questions

- Each FAQ should be a subsection with a question, like `### Why is this a good idea?`. This makes for easy linking.

### Implementation plan

- Break the work into small, independently mergeable steps.
- Each step should leave the codebase in a working state.
- Describe the tests you will use to verify that this step was successful.
- Use subsections for each step, with a heading like `### Step 1: Describe the step`

## Example: good vs. bad

Bad:
> This powerful new predicate system will unlock incredible flexibility for plugin authors, enabling them to express complex activation conditions with ease.

Good:
> Add a `shell(...)` predicate that runs a command and holds when exit code is 0.
>
> ```toml
> predicates = ["shell(which cargo-nextest)"]
> ```
