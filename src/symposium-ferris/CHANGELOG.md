# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/symposium-dev/symposium/compare/symposium-ferris-v1.0.0...symposium-ferris-v1.0.1) - 2025-12-31

### Other

- update Cargo.toml dependencies

## [1.0.0](https://github.com/symposium-dev/symposium/compare/symposium-ferris-v1.0.0-alpha.2...symposium-ferris-v1.0.0) - 2025-12-30

### Other

- update Cargo.toml dependencies

## [1.0.0-alpha.2](https://github.com/symposium-dev/symposium/releases/tag/symposium-ferris-v1.0.0-alpha.2) - 2025-12-30

### Added

- thread cwd through to cargo metadata commands

### Other

- upgrade sacp crates to alpha.4, sparkle-mcp to 0.4.1
- use the ability to disable tools
- move mcp out from module
- move rust_researcher to top-level
- put mcp under the tool, not centralized
- rename rust module to crate_sources
- simplify sig with impl trait
- [**breaking**] flatten public API of crate
- consolidate Ferris MCP server with configurable tools
- rename symposium-eg to symposium-ferris with MCP server
