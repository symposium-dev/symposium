# Plugin discovery, marketplaces, and trust

**Status:** Draft for discussion. No implementation is claimed by this RFD.

## TL;DR

- Distribute whole plugins in the Open Plugin format through existing package managers and registries. Deprecate other authoring formats; consider a directory of skills as a convenience input.
- Expose Symposium (Global) and Symposium (Local) marketplaces through agent-native interfaces.
- Use predicates to discover relevant plugins, not to filter individual components of an installed plugin.
- Separate availability from enablement. Auto-enable defaults to on, subject to trust policy and explicit user disablement.
- Let configured plugin managers vouch with `require`, `allow`, `warn`, or `deny`. Check mandatory policy against the host's actual state.

## Motivation

A project depending on a library should be able to discover that library's plugin in its agent's plugin browser. Whether the plugin runs is a separate decision: a user may prefer manual selection, a team may recommend it, or an organization may forbid its version.

Today Symposium's [discovery and enablement flow](../../design/important-flows.md#dependency-enablement) mixes these decisions. Registries are trust roots, dependency plugins require consent, and predicates filter the extensions Symposium delivers. Native plugin browsers introduce another place where users make installation and enablement decisions.

This proposal makes Symposium responsible for discovery, policy, and synchronization, and uses the host's plugin machinery for installation and loading. Shipping whole plugins also lets authors distribute new host capabilities without Symposium interpreting every component.

The [work organization discussion](https://symposium-dev.zulipchat.com/#narrow/channel/530148-general/topic/Work.20organization/with/637503094) identified conflicting expectations around global predicates, approval versions, and organization policy. The [plugin installation discussion](https://symposium-dev.zulipchat.com/#narrow/channel/530148-general/topic/2026-10-19.2FInstalling.20via.20open.20plugins/with/636117437) raised marketplace collisions, subdirectory startup, and session reload limitations. The [automated audit discussion](https://symposium-dev.zulipchat.com/#narrow/channel/530148-general/topic/Automated.20plugin.20audits/with/637496700) connects those decisions to approval policy. This RFD brings them together; unresolved choices are called out below.

## Change in a nutshell

Suppose a plugin manager recommends `rust-helper` for projects depending on a particular Rust crate and vouches `allow` for its resolved version.

1. In a matching workspace, synchronization puts `rust-helper` in Symposium (Local).
2. With auto-enable on, Symposium installs and enables it through the host. With auto-enable off, the plugin remains available in the native browser.
3. Disabling it in the browser records an optional-plugin choice that subsequent syncs preserve.
4. A later `deny` removes it from Symposium's installable catalog and requires reconciliation of any enabled copy Symposium manages.

Explicit selection works similarly:

```bash
cargo agents use rust-helper          # make available in this workspace
cargo agents use --global rust-helper # make available across workspaces
```

`use` no longer unconditionally means enable. It selects marketplace membership; policy, acknowledgments, auto-enable, and native choices determine activation. These are proposed semantics, not a description of the current CLI.

See the [proposed user reference](./proposed-reference.md) for the user-facing behavior and the [host integration plan](./host-integration.md) for experiments needed before implementation.

## Detailed plans

### Plugin packages and distribution

“Open Plugin format” here means the [Agent Plugins format](https://agent-plugins.org/) already referenced by the [interoperability RFD](../agent-plugins/README.md), rather than a new Symposium format. A plugin is the distribution and enablement unit. Cargo, npm, PyPI, and configured registries supply packages through the [registry-centric PM architecture](../registry-centric-plugins/README.md); this does not imply all ecosystem adapters are implemented today.

Authors publish a portable plugin package. Symposium may supply host-specific registration metadata, but does not reconstruct a different set of skills, hooks, or other components for each workspace. Host-specific extensions inside a portable package remain subject to that host's capabilities; a portable container does not make every component portable.

Deprecate `Symposium.toml` and package metadata as plugin-content authoring formats, and direct ingestion of other host-specific plugin formats as long-term alternatives. This does not deprecate Symposium's own configuration or the PM interface. Discovery metadata and vouches belong to the discovery/policy layer, not an alternative extension manifest.

A directory of skills may remain a shorthand that Symposium wraps into one plugin. Its layout, identity, and version rules are an open decision; it must follow the same policy and enablement path. It must not restore per-skill activation predicates or bypass warnings because it lacks a manifest.

### Global and workspace-local availability

Symposium maintains two conceptual catalogs:

| Catalog | Membership | Scope |
|---|---|---|
| Symposium (Global) | `use --global` selections and explicitly global PM recommendations | Available across workspaces |
| Symposium (Local) | Local `use` selections and relevant dependency/predicate/PM discoveries | One workspace |

Predicates decide whether an automatically discovered plugin belongs in the local catalog. Explicit `use` selects a plugin even if discovery would not recommend it. Global selections do not become conditional on whichever workspace happened to synchronize last. A recommendation that needs workspace predicates belongs in the local catalog.

This chooses the discovery-only interpretation discussed on November 4 over the earlier suggestion that global installations remain predicate-gated per workspace. Once installed, a plugin exposes its whole contents; Symposium no longer evaluates skill-level predicates at dispatch time. Authors needing different applicability should split their plugins or place guidance in the skills themselves.

Removing a discovery match withdraws that automatic local membership. An explicit selection remains. When no membership remains, reconciliation removes Symposium-owned enablement and registration for that scope; it does not delete unrelated host installations. Native selection of a discovered plugin must be recorded as a durable local selection before this cleanup, so later predicate changes do not undo that choice.

Global and local membership in the same canonical plugin must not cause duplicate execution. The adapter must map both to one effective host instance or report an unsupported scope/version conflict. Workspace policy can prohibit a globally selected plugin locally; a host that cannot honor that restriction cannot claim support for that policy.

### Vouches and authority

A vouch identifies its issuer, the canonical plugin identity (including its package manager/source), the versions it covers, its scope, its policy, and a reason. Resolve the package version before applying vouches. A name alone must not allow an approval for one registry's plugin to authorize another registry's same-named plugin.

Only managers configured as policy authorities can issue effective vouches. Merely discovering a manager, registry, or plugin in a project cannot give it authority to approve that project. Discovery and authority are separate even when the same installed manager provides both.

| Policy | Meaning |
|---|---|
| `require` | Must be enabled, independent of optional user preferences |
| `allow` | May be enabled automatically when auto-enable is on |
| `warn` | Requires explicit opt-in and warning acknowledgment |
| `deny` | Must not be available for installation or enabled |

All discovery routes, including explicit `use`, trusted registries, workspace content, and chained sources, pass through the same policy resolution. An organization allowlist must cover all of them. A plugin may remain in diagnostic output with the reason it was denied; it is not an installable catalog entry.

Managed organization policy needs a trust source and configuration precedence that a project cannot replace. Ordinary editable `~/.symposium` configuration does not provide enforcement against its owner. Integrating an administrator-controlled policy source is a separate implementation requirement, not a security guarantee supplied by these four labels.

### Resolving policies

The proposal establishes these rules:

1. `require` plus `deny` is a conflict. Report the issuers and affected plugin/version; do not proceed as if policy was satisfied.
2. Otherwise, any `deny` forbids the plugin, regardless of a team recommendation or user acknowledgment.
3. `require` mandates enablement and cannot be canceled by auto-enable being off or by user disablement.
4. `allow` permits default enablement when auto-enable is on, unless an explicit optional disablement exists.
5. `warn` requires explicit acknowledgment before enablement.

The attached proposal leaves mixed `allow`/`warn` and unvouched plugins unresolved. **Recommended decisions for discussion:** a warning cannot be canceled by an `allow`; unvouched plugins behave as `warn`. For `require` plus `warn`, retain the acknowledgment requirement and report an unsatisfied requirement until acknowledged. This avoids silently suppressing a warning while retaining the mandatory policy. These recommendations need agreement before implementing the policy table.

Compute one effective decision, with its reasons, for catalog publication, enablement, and hook validation. Do not resolve policies independently in each path.

### Approvals, versions, and audits

A warning acknowledgment applies to a canonical plugin and an exact version or an explicit version range, for one workspace or globally. Global acknowledgment is reusable across workspaces, but neither kind overrides `deny`. Proposed default: approve the exact resolved version; expanding to a range is a separate explicit choice.

Version approval and audit evidence answer different questions. A version range says which releases the user is willing to authorize. An audit digest says which content was inspected. A patch release with changed content can be within an approved range while needing a new audit; a major release with identical content can still be outside that range.

An audit manager can translate its evidence into vouches. This RFD does not equate an acknowledgment with a successful audit, or require a particular scanner. The digest-based audit work discussed in Zulip must agree on what is hashed, how evidence expires, and whether changed evidence creates a new warning that invalidates an earlier acknowledgment. Those remain open rather than silently choosing version or digest semantics for that work.

Revocation prevents future activation based on that acknowledgment and triggers reconciliation of any enabled plugin that depended on it. Cached installation is not continuing permission. Approval/revocation CLI syntax and storage are still to be designed; `use` alone is not warning acknowledgment.

### Automatic enablement and explicit choices

Auto-enable defaults to **on** for plugins whose effective policy permits it. Turning it off affects optional automatic activation, not catalog discovery, required plugins, or the warning gate. This preference is distinct from today's `[plugins] auto-enable` list and needs an explicit configuration migration.

Native enablement is an explicit user selection subject to policy. Native disablement of an optional plugin survives repeated sync, restart, and discovery refresh. Disabling a required plugin is a policy violation, not a new optional preference.

Persist canonical selection and disablement decisions separately from generated host settings and warning acknowledgments. Compare observed host state with the last Symposium-written state; absence of a setting alone is not proof of user intent. Where a host cannot expose the distinction, report the ambiguity and avoid repeatedly enabling an optional plugin the user turned off.

Existing enablements and declines must migrate as explicit choices. Changing the default must not turn previously declined plugins on or convert old unversioned consent into unlimited version approval.

### Reconciliation and actual state

For each sync:

1. Discover candidates and resolve package identities and versions.
2. Collect applicable vouches and acknowledgments, then compute effective policy.
3. Publish eligible global and local catalogs.
4. Compute desired enablement from policy, user choices, and auto-enable.
5. Reconcile Symposium-owned native registrations and installation/enablement state.
6. Inspect actual host state and report enabled, disabled, pending reload, unknown, or policy violation.

Catalog presence, installed files, configured enablement, and loaded session state are different observations. Writing configuration is not evidence that the running agent loaded a plugin.

Preserve unrelated settings and marketplace entries. Use stable internal marketplace identities that distinguish workspaces, even if their display labels are both Symposium (Local). Avoid concurrent syncs losing one another's changes. Failed discovery is not an empty catalog: retain the last known state and report the failure rather than reaping installations. Failure to obtain policy must not authorize new activation; mandatory-policy status remains unknown until checked.

### Hooks and enforcement limits

Symposium's own integration hooks check required and denied plugins independently of the required plugins' hooks. Otherwise disabling the plugin would also disable its validator. Use actual host state where available, and distinguish configuration from what the session loaded.

A required plugin disabled through the host produces an error naming the requirement and a recovery action. A denied plugin still enabled is also a violation. Optional disablement is normal. A newly installed required plugin awaiting reload is an unsatisfied requirement, not successful activation.

Each host adapter must prove which hook can observe state and which response actually stops work. A diagnostic shown to the model is not enforcement. Where the host cannot expose or enforce the needed state, report that limitation; do not claim mandatory-policy compliance. Hooks on a user-controlled machine are not an administrator boundary by themselves.

### Relationship to earlier designs

This proposal retains package-manager identities, registry distribution, and dependency discovery from the registry-centric RFD. It changes predicates from activation gates to discovery gates and replaces unconditional registry trust with explicit policy resolution.

It revises the interoperability RFD's compilation of filtered Symposium manifests: the authored Open Plugin package becomes the unit delivered through native marketplaces. Compatibility adapters may produce host metadata, but long-term support does not include several parallel plugin authoring formats.

The destination is host-native loading of plugin components. Removing existing hook translation, installation commands, subcommands, or MCP/meta-server paths requires a migration plan for each capability; no component should silently disappear. Whether an MCP meta-server remains an explicitly packaged plugin capability is a separate decision. Agents without suitable native plugin support need a documented capability boundary before the legacy delivery path is removed.

## Frequently asked questions

### Why keep predicates if hosts already have marketplaces?

Predicates let Symposium populate a useful local catalog from project dependencies and context. They need not modify a plugin's contents or compete with the host's native enablement controls.

### Why deprecate other plugin formats?

One authoring contract reduces the number of schemas and translations Symposium maintains. Host metadata adaptation is still necessary, but it is not a promise to import every host's evolving plugin language. A skills-directory shorthand can preserve the simplest author workflow without becoming another full format.

### Does turning auto-enable off require a `use` command for every discovery?

No. Eligible discoveries remain browsable and can be selected through the host. The attached proposal keeps automatic activation for allowed plugins by default, rather than requiring `use` for every recommendation.

### Can an organization prohibit plugins installed outside Symposium?

Not merely by hiding them from Symposium's catalogs. Host-managed policy and effective-state inspection are needed to cover other installation routes. The implementation must state whether its policy applies only to Symposium-managed plugins or can be enforced across the host.

### Which decisions still need discussion?

- Confirm mixed-policy precedence, the default for unvouched plugins, and acknowledgment invalidation after new audit evidence.
- Define the authoritative policy configuration and vouch transport, including unavailable or revoked authorities.
- Decide whether to support a skills-directory shorthand and how to identify/version it.
- Set canonical workspace boundaries for nested and polyglot projects, and behavior when a host starts in a subdirectory.
- Validate native intent detection, scope/version collisions, live activation, and mandatory-policy enforcement per host.
- Agree on deprecation milestones and the capabilities/support matrix required before removing legacy formats and delivery.

## Implementation plan and status

### Step 1: Validate native host behavior

- [ ] Run the [host integration experiments](./host-integration.md) in isolated configurations, recording host versions and actual loaded plugins.

Verify fresh installation, refresh/restart, project isolation, subdirectory startup, native enable/disable, existing catalog coexistence, and required-plugin violations. Keep production delivery unchanged while these establish the supported contract.

### Step 2: Separate discovery and decisions

- [ ] Add a shared candidate/identity model and distinct selection, acknowledgment, and desired-state records behind an opt-in path.

Test matching/nonmatching workspaces, global selection stability, duplicate identities, explicit selections surviving predicate changes, and idempotent migration of existing use/disable state. Existing behavior remains the default until rollout.

### Step 3: Add vouches and policy resolution

- [ ] Agree on the open policy choices and implement the authority boundary and single decision engine.

Table-test every policy combination, scoped/versioned acknowledgment, revocation, unvouched content, and unavailable managers. Test that a project cannot approve itself and that registry, chained, and explicit-use routes cannot bypass prohibitions.

### Step 4: Publish and reconcile marketplaces

- [ ] Add Claude Code and Codex adapters using the experimentally verified mechanisms.

Test preservation of unrelated files/settings, native disablement across repeated syncs, concurrent workspaces, safe failure/retry, duplicate global/local membership, and installation without activation. End-to-end tests must ask the host what actually loaded.

### Step 5: Validate mandatory policy in hooks

- [ ] Add host-specific actual-state checks and actionable violation/reload reports.

Test disabling a required plugin, enabling a denied plugin, unknown state, and reload-pending state. Exercise real hook wire responses and stdout cleanliness so a reported violation is not mistaken for enforcement.

### Step 6: Migrate authoring and defaults

- [ ] Publish migration guidance, decide the skills-directory exception, and warn on legacy formats before removing support in an announced release.

Inventory existing components; test representative plugin migrations for equivalent behavior and explicit diagnostics for unsupported features. Preserve prior declines and enablements. Update the user reference, design chapters, and agent support matrix when the new path becomes the default.
