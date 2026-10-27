---
name: fixture-store
description: Use when reading or writing data with the fixture-store crate.
---

`Store::put` buffers writes in memory. They reach disk only when `Store::flush`
is called, and a `Store` dropped without flushing loses them without an error.
Call `flush()` before returning from any function that writes.
