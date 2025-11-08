# Project

## Introduction

@md/introduction.md

## Documentation

Design documentation is tracked in the mdbook:

@md/SUMMARY.md

The implementation overview chapter is particularly useful for highlighting the major components:

@md/design/implementation-overview.md

## Work Tracking

We track all pending work in GitHub issues on the symposium/symposium repository. When starting work on an issue that is complex enough to merit a new chapter in the documentation, create design documents initially in the `md/work-in-progress/` section of the mdbook. These documents are refined as work progresses and eventually moved to the main book sections once the work is complete. Post updates on tracking issues when checkpointing work.

## Instructions

Agent MUST follow the following guidance:

* **Update design documentation**: Update the mdbook chapters in `md/design` as appropriate so that they are kept current. This will help both you and future agents to remember how things work.
* **Check that everything builds and don't forget tests**: After making changes, remember to check that the typescript + swift + Rust code builds and to run tests.
* **Auto-commit completed work**: After completing a series of related changes, automatically commit them with a descriptive message. This makes it easier for the user to review progress.
* **Co-authorship**: Include "Co-authored-by: Claude <claude@anthropic.com>" in commit messages to indicate AI collaboration.
