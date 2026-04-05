# Project

## Introduction

@md/introduction.md

## Documentation

Design documentation is tracked in the mdbook:

@md/SUMMARY.md

The implementation overview chapter is particularly useful for highlighting the major components:

@md/design/implementation-overview.md

## Artifact assembly

The Claude Code plugin is assembled from multiple source locations by `symposium-artifacts.toml`.
The canonical `symposium.sh` lives at `agent-plugins/symposium.sh` and is copied into the
assembled plugin by the artifact steps. Do NOT duplicate it into `agent-plugins/claude-code/scripts/`.

## Instructions

Agent MUST follow the following guidance:

* **Check common issues first**: Before starting a coding task, review `md/design/common-issues.md` for recurring bug patterns that may apply to your work.
* **Update design documentation**: Update the mdbook chapters in `md/design` as appropriate so that they are kept current. This will help both you and future agents to remember how things work.
* **Check that everything builds and don't forget tests**: After making changes, remember to check that the typescript + swift + Rust code builds and to run tests.
* **Co-authorship**: Include "Co-authored-by" with your agent identifier (e.g., "Claude <claude@anthropic.com>") to indicate AI collaboration.
