# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/symposium-dev/symposium/releases/tag/symposium-sdk-v0.1.0) - 2026-07-22

### Other

- Load crates as plugins via [[plugins]] chained references
- Workspace plugins: the workspace's own dirs define plugins
- Merge pull request #252 from kurasaiteja/tejakura-custom-predicate-jsonl
- switch custom predicate stdout to JSONL
- Make WorkspaceCrate and LoadedWorkspace non-exhaustive
- Address PR review feedback
- Use symposium-sdk types directly instead of re-exporting
- Require SymposiumDirs when constructing WorkspaceDeps
- Replace ad-hoc WorkspaceDeps fields with SymposiumDirs
- Add SymposiumDirs to symposium-sdk for shared path resolution
- Add WorkspaceDeps with in-process and disk caching
- Introduce symposium-sdk crate and refactor main crate to use it
