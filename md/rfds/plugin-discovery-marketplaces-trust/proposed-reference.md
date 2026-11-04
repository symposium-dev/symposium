# Proposed: Discovering and enabling plugins

This page describes proposed behavior. The preference names, approval commands, and output formatting are not a committed CLI schema.

## Make a plugin available

```bash
cargo agents use rust-helper
cargo agents use --global writing-helper
```

The first command adds a plugin to Symposium (Local) for this workspace. The second adds one to Symposium (Global), available across workspaces. Global selections do not change when another project's dependencies change.

Symposium also discovers local plugins from project dependencies and plugin-manager recommendations. Predicates control these discoveries. You can select a plugin explicitly even when it was not recommended for this project.

## Choose what runs

Browse Symposium's marketplaces through your agent's native plugin interface. Availability does not mean the plugin is enabled or loaded into the current session.

Auto-enable is on by default. Turn it off if you prefer to select optional plugins yourself. Your explicit disablement of an optional plugin is preserved by later synchronization.

| Effective policy | What happens |
|---|---|
| Required | Must be enabled; disabling it reports a policy violation |
| Allowed | Enabled automatically when auto-enable is on, unless you disabled it |
| Warning | Requires your acknowledgment before it can be enabled |
| Denied | Cannot be installed through Symposium or enabled under its policy |

Mixed policies and the unvouched default are still under discussion in the [RFD](./README.md#resolving-policies). Auto-enable and explicit selection cannot override a denial or acknowledge a warning for you.

## Acknowledge a warning

An acknowledgment names a plugin, an exact version or an explicit version range, and either this workspace or all workspaces. The proposed default is the exact resolved version. A global acknowledgment avoids repeating that acknowledgment in each workspace; it does not globally select the plugin.

An update outside an approved range needs a new acknowledgment. Audit requirements can additionally depend on the content that was inspected. Revocation removes the acknowledgment's authority for future activation and causes Symposium to reconcile any enabled plugin relying on it. The acknowledgment and revocation interfaces will be specified before implementation.

## Synchronize and inspect state

```bash
cargo agents sync
cargo agents status
```

Status should distinguish catalog availability, installed files, desired enablement, and actual loaded state, with policy reasons. If the host needs a reload or restart, Symposium reports that action. It does not report success merely because configuration was written.

A required plugin that is disabled or awaiting reload is an unsatisfied requirement. Follow the reported recovery action. If the host cannot verify a mandatory policy, status reports the limitation.

## Author a plugin

Publish a whole Open Plugin package through a configured registry or package manager. Its components are delivered together; Symposium does not filter individual skills by workspace predicates.

Other plugin authoring formats will be deprecated with migration guidance. A directory of skills may remain a convenience input; that exception is still under discussion. Existing Symposium-specific components need an explicit migration path before legacy support is removed.
