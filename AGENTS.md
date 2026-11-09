# Project

## Introduction

@md/introduction.md

## Documentation

Design documentation is tracked in the mdbook:

@md/SUMMARY.md

The module structure and important flows chapters are particularly useful for understanding the codebase:

@md/design/module-structure.md

@md/design/important-flows.md

## Instructions

Agent MUST follow the following guidance:

* **Check common issues first**: Before starting a coding task, review `md/design/common-issues.md` for recurring bug patterns that may apply to your work.
* **Update design documentation**: Update the mdbook chapters in `md/design` as appropriate so that they are kept current. This will help both you and future agents to remember how things work.
* **Check that everything builds and don't forget tests**: After making changes, remember to check that the typescript + swift + Rust code builds and to run tests.
* **Keep tests portable across CI platforms**: Tests run on Linux (glibc and musl), macOS, and Windows. Before changing subprocess or filesystem tests, check `.github/workflows/ci.yml` and the Windows portability notes in `md/design/common-issues.md`. Reuse portable helpers, resolve shells through `PATH` (`sh`, not `/bin/sh`), and review fixtures for quoting, path separators, and platform-specific executable behavior.
* **Verify platform-specific fixes**: Passing local tests does not validate another operating system. After publishing a CI fix, inspect the affected platform's CI result and explicitly report any verification still pending.
* **Co-authorship**: Include "Co-authored-by" with your agent identifier (e.g., "Claude <claude@anthropic.com>") to indicate AI collaboration.
