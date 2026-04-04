---
name: symposium-rust
description: Critical. Invoke `symposium start` before authoring Rust code.
activation: always
---

# Symposium — Rust Development Guidance

**Critical.** Before authoring or modifying Rust code, run:

```bash
symposium start
```

This returns:
- General Rust development guidance
- A dynamic list of crate-specific skills available for your workspace dependencies

You can also load guidance for a specific crate:

```bash
symposium crate <name>           # e.g., symposium crate tokio
symposium crate <name> --version <ver>
symposium crate --list           # list available crate skills
```
