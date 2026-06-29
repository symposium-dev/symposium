# Code review instructions

## Rust coding practices

Watch for these patterns:

- **Document the destination, not the journey**: Comments and documentation should describe what the code does now, not what it used to do or why it changed. Avoid references to past behavior, removed code, or migration history.
- **Exhaustive is better**: Prefer exhaustive matches over wildcard (`_`) arms — when new variants are added, the compiler should force a decision. Similarly, prefer destructuring structs (`let Foo { a, b, .. } = x`) over field access (`x.a`, `x.b`) when the code is likely to need updating as fields are added.
- **Private is better**: Flag items that are more public than necessary. Default to private; use `pub(crate)` or `pub(super)` before `pub`. The library surface of symposium is mostly internal — `symposium-sdk` is the exception.
- **Focus on the now**: Flag backwards-compatibility shims, re-exports of removed items, `#[deprecated]` stubs, or dead code left "just in case." Symposium is primarily an application; unused abstractions are waste, not investment.

## RFD compliance

Check whether the PR's changes relate to an active RFD (listed under "Accepted" in `md/SUMMARY.md`, with corresponding content in `md/rfds/`). Completed RFDs (under "Completed") are not active — ignore those.

If the PR does relate to an active RFD:

1. **Implementation plan updates**: Verify that the RFD's "Implementation plan and status" section is updated in this PR to reflect the work done (e.g., checking off items, adding new steps discovered during implementation).
2. **Design consistency**: Compare the PR's approach against the RFD's design. Flag deviations that aren't documented in the RFD's status section. Minor deviations are fine if noted; undocumented major departures should be called out.
3. **Scope alignment**: If the PR introduces changes outside the RFD's stated scope, note this — it may belong in a separate PR or the RFD scope may need updating.

If the PR does not relate to any active RFD, skip these checks.
