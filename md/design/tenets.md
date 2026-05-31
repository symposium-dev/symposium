# Tenets

Design principles that guide symposium's architecture. When in doubt, these break ties.

## Unobtrusive

Symposium should never be a reason to opt out. Using it should be non-disruptive to existing workflows:

- Existing projects should be able to adopt symposium without restructuring.
- Never dirty the user's repo with unexpected files or diffs or require users to manually edit `.gitignore`.
- Avoid adding "symposium-specific" files or modifications in project repositories (when possible).

## Prefer existing standards over our own

When there's an existing mechanism that works, use it rather than inventing a new one. Adopt agent conventions, standard file layouts, and community norms wherever possible. Symposium's own canonical format exists only where no cross-agent standard exists.

## Union, not least-common-denominator

Symposium's plugins should be able to take full advantage of what agents can do. We aim for interoperability but we also let plugin authors opt into agent-specific formats or capabilities.

## Vendor neutral, interoperable

Plugins and repository provide functionality; users pick their agent. Symposium provides the bridge, exposing plugin functionality in whatever way is requested by an individual user.

## Safety

Avoid exposing users to fresh risk. Plugins run code on the user's machine — symposium should make it easy to audit, constrain, and revoke. The central repository requirement exists to prevent supply-chain attacks until we have better decentralized trust mechanisms.

## Empower the ecosystem

Crate authors should be able to ship agent extensions independently, without waiting for central approval or coordination beyond the initial plugin registration. Once registered, updates flow through normal crate publishing. The symposium project's role is infrastructure, not gatekeeping.
