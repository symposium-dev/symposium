# Cargo

Cargo provides tools for running common cargo commands with compressed output, helping your agent save context and focus on what matters.

## Quick Reference

| What | How |
|------|-----|
| Build | Agent uses cargo build tool |
| Run | Agent uses cargo run tool |
| Test | Agent uses cargo test tool |

## How It Works

Instead of running raw cargo commands through bash, your agent can use Cargo's specialized tools. These tools:

- **Compress output** - Filter and summarize cargo's verbose output to highlight errors, warnings, and key information
- **Save context** - Reduce token usage by removing noise, leaving more room for actual problem-solving
- **Focus attention** - Present the most important output first so the agent can quickly identify issues

## Why Not Just Bash?

Raw `cargo build` output can be verbose, especially with many dependencies or detailed error messages. The Cargo extension processes this output to extract what the agent actually needs to see, making it more efficient at diagnosing and fixing issues.
