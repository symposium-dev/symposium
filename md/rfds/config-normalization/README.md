# Configuration parsing and normalization

## TL;DR

- Parse TOML into raw structs that describe the accepted file syntax.
- Convert raw structs into normalized runtime structs in explicit validation steps.
- Runtime code consumes normalized structs, not TOML-shaped structs.
- Commands that edit user-owned TOML preserve formatting through a CST-aware editing path.
- This RFD does not change user-facing configuration syntax.

## Motivation

Symposium currently uses more than one pattern for TOML parsing.

The user configuration in `config.rs` mostly mirrors the file shape. `Config`
contains fields like `defaults` and `plugin_source`, and methods such as
`Symposium::plugin_sources()` compute the effective view used by runtime code.

Plugin manifests in `plugins.rs` are closer to a raw-to-normalized pipeline.
`RawPluginManifest` deserializes the manifest, then `validate_manifest()`
produces a validated `Plugin`. During that step, inline installation references
are promoted into named installations and plugin-level `crates` plus
`predicates` are merged into one `PredicateSet`.

Some manifest types also normalize during deserialization. For example,
`SkillGroup` and `PluginMcpServer` implement custom `Deserialize` so their
`crates` and `predicates` fields are merged before validation sees them.

The registry-centric plugin work adds more syntax with separate file and runtime
shapes: `[[plugins]]`, `source.*`, `where.*`, provenance, discovery policy, and
plugin defaults. Before adding those pieces, we should make the parsing boundary
consistent.

## Change in a nutshell

### separate "raw" from "normalized", prefer derived serde traits

Every parsed TOML file has a raw root struct and a normalized runtime
representation. The raw tree reflects the file shape; the normalized tree
reflects the runtime model.

The root type used to deserialize a TOML file is always raw. Nested structs
should also be raw when they represent TOML sections or entries that need
normalization before runtime use. Field-level syntactic types, such as a parsed
predicate expression or source specifier, may be non-raw when their invariants
are local to that value.

This keeps the overall parse path consistent: deserialize the file through a raw
root, then validate into a normalized runtime root. It still permits small
non-raw field types where a separate raw/runtime split would only add ceremony.

Raw structs should use derived `Deserialize` where possible. We recommend
denying unknown fields to catch typos.

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkillGroup {
    #[serde(default)]
    crates: Option<CrateList>,
    #[serde(default)]
    predicates: PredicateSet,
    #[serde(default)]
    source: PluginSource,
}
```

There is then a distinct normalized struct used throughout the codebase:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SkillGroup {
    pub predicates: PredicateSet,
    pub source: PluginSource,
}
```

And finally inherent methods to convert from the "raw" version to the normalized one:

```rust
impl RawSkillGroup {
    fn validate(self) -> anyhow::Result<SkillGroup> {
        Ok(SkillGroup {
            predicates: PredicateSet::merged(self.crates, self.predicates),
            source: self.source,
        })
    }
}
```

### rewrite with toml-edit

For files that Symposium rewrites, keep a fourth concern separate: the concrete
syntax tree used to preserve user formatting.

```rust
struct EditableConfig {
    document: toml_edit::DocumentMut,
    config: Config,
}
```

Runtime code should not depend on the editable representation. Editing commands
load the document, parse or validate the relevant parts, apply localized edits
to the document, and write the document back.

## Detailed plans

### Raw structs

Raw structs describe accepted TOML syntax. Every TOML file type should have a
raw root struct, even if some fields currently convert one-to-one into runtime
values. Raw structs should:

- derive `Deserialize` where possible;
- use `#[serde(deny_unknown_fields)]` unless the format intentionally accepts
  extension fields;
- keep aliases, deprecated fields, and migration-only fields visible at the
  parsing boundary;
- avoid runtime-only derived fields.

Raw structs may contain other raw structs, field-level syntactic types, and
plain scalar/container values. They should not contain normalized runtime
structs for nested TOML sections that still need cross-field validation.

Raw structs should prefer Serde's derived enum representations for syntactic
unions. Use `#[serde(flatten)]`, untagged enums, or externally tagged enums when
they describe the TOML shape directly. Custom `Deserialize` implementations
should be rare and limited to cases that derived Serde cannot express clearly.

Raw structs should not perform semantic normalization that depends on sibling
fields or later validation context.

### Current custom deserializer audit

The current custom `Deserialize` implementations fall into these categories:

| Type | Current role | Decision |
|------|--------------|----------|
| `CrateList` | Accepts `crates = "serde"` or `crates = ["serde"]`, then parses crate-atom strings into predicates. | Split the TOML shape from the grammar parsing. The string-or-list shape can be expressed as an untagged raw enum; crate-atom parsing remains field-level parsing. |
| `Predicate` | Parses one predicate expression string, such as `crate(serde)` or `any(crate(a), crate(b))`. | Keep as a field-level syntactic parser. The function-call grammar is not TOML shape normalization. |
| `PredicateSet` | Deserializes a list of predicate expression strings by delegating to `Predicate`. | Keep near the predicate parser. It may be replaceable with `#[serde(transparent)]`, but it is not the main normalization problem. |
| `PluginMcpServer` | Reads `crates`, `predicates`, and a flattened MCP server, then merges activation fields into one `PredicateSet`. | Move to `RawPluginMcpServer::validate`. This is semantic normalization across sibling fields. |
| `PluginSource` | Accepts string and table forms, rejects removed table fields, enforces mutually exclusive table keys, and produces a runtime enum. | Move semantic checks into a raw source type with derived Serde where possible. Use an untagged enum or flattened table representation for the accepted TOML shapes. |
| `SkillGroup` | Reads `crates`, `predicates`, and `source`, then merges activation fields into one `PredicateSet`. | Move to `RawSkillGroup::validate`. This is semantic normalization across sibling fields. |

This audit is intentionally about custom deserializers, not every validation
step. Existing raw structs such as `RawPluginManifest`, `RawHook`, and
`RawSubcommand` already follow the desired shape: Serde reads TOML fields, then
validation functions produce normalized runtime values.

### Normalized runtime structs

Runtime structs describe the validated model used by sync, hooks, subcommands,
plugin loading, and reporting. They should:

- have fields that correspond to runtime concepts;
- avoid preserving aliases or deprecated syntax;
- store derived relationships when that simplifies runtime code;
- be serializable for tests, reports, or debug output when useful.

For example, a normalized plugin should have one `PredicateSet` for activation
even if the source TOML can express that predicate set through more than one
field.

### Validation and conversion

Conversion from raw to normalized structs is explicit. Prefer inherent methods
on raw structs:

```rust
impl RawPluginManifest {
    fn validate(self, sym: &Symposium) -> anyhow::Result<Plugin> {
        // ...
    }
}
```

Context-free conversions use `fn validate(self)`. Conversions that need
configuration directories, cache directories, the active `Symposium`, workspace
state, or a registry resolver take those values as ordinary parameters. Named
helper functions are appropriate for shared validation logic or when a
conversion spans multiple raw values.

The conversion step handles:

- merging syntactic sugar into runtime fields;
- conflict checks, such as mutually exclusive fields;
- duplicate-name checks;
- promotion of inline entries into named entries;
- migration errors and migration hints;
- cross-field validation.

Serde errors should be limited to syntax shape problems: unknown fields, wrong
types, missing required fields, and invalid field-level enum forms.

### CST-preserving edits

Parsing and editing are separate concerns.

`toml::from_str` plus Serde is appropriate when Symposium only needs to read a
file. It does not preserve comments, ordering, whitespace, or original spelling.

Commands that modify user-owned TOML should use a CST-aware path based on
`toml_edit`. This applies to commands such as future plugin-management commands
that add or remove entries from user config.

The CST-aware path should still validate through the same raw-to-normalized
logic before relying on the edited file.

This is red-green testable. A test should start with a user config containing
comments, non-default ordering, and unrelated tables. It should apply a narrow
edit through the config-editing API, then assert that:

- the intended semantic change is present;
- unrelated comments are still present;
- unrelated table and key ordering is unchanged;
- the resulting file parses through the normal raw-to-normalized validation
  path.

### Accepted aliases

Aliases should be rare. Each accepted spelling increases documentation,
validation, and migration surface area.

When an alias is accepted, the raw struct should represent both spellings and
the conversion step should normalize them into one runtime field. If both
spellings are present and conflict, conversion should report a semantic error.

## Frequently asked questions

### Does this change any user-facing syntax?

No. This RFD describes an internal organization rule. Existing accepted syntax
continues to parse unless a separate RFD removes or migrates it.

### Why not keep runtime structs shaped like TOML?

Runtime code should not have to know every accepted spelling or deprecated
field. A normalized runtime model keeps validation at the boundary and makes
later code depend on stable concepts.

### Why not normalize everything inside custom `Deserialize` implementations?

Custom deserializers are useful for local field-shape problems, but they hide
normalization inside parsing. That makes it harder to report semantic errors
with context and harder to keep a consistent boundary between file syntax and
runtime model. They are also harder to read: a raw struct lets the reader infer
the expected TOML shape from the Rust fields and Serde attributes.

### Does this require preserving the exact TOML CST everywhere?

No. CST preservation is only needed for commands that rewrite user-owned TOML.
Read-only paths can parse through Serde and discard formatting.

## Implementation plan and status

### Step 1: Document the rule

Add this RFD and link it from the mdbook RFD section.

- [x] RFD added.

### Step 2: Refactor plugin manifest parsing without behavior changes

Use the custom deserializer audit above to move semantic normalization out of
custom `Deserialize` implementations. Start with `SkillGroup` and
`PluginMcpServer` because they already perform simple `crates` plus
`predicates` normalization. Then move `PluginSource` semantic checks behind a
raw source type with derived Serde where possible.

Verification:

- targeted plugin manifest parsing tests cover the moved cases;
- `cargo fmt`;
- `cargo clippy --all --workspace`;
- `cargo test --all --workspace`
- Existing plugin manifest parsing tests still pass.

- [x] Custom deserializer audit.
- [ ] Behavior-preserving parser refactor.

### Step 3: Refactor user config parsing without behavior changes

Introduce raw config structs where doing so clarifies the distinction between
file shape and effective runtime view. Keep existing user-facing config syntax.

Verification:

- `cargo test --all --workspace`
- Existing config parsing tests still pass.

- [ ] Behavior-preserving config parser refactor.

### Step 4: Add CST-aware editing helpers when needed

When the first command needs to edit user config while preserving formatting,
introduce a small `toml_edit`-based helper. Do not introduce this before there
is a command that needs it.

Verification:

- a red-green test with comments, non-default ordering, and unrelated tables;
- assertions that the intended semantic change is present;
- assertions that unrelated comments and ordering survive the edit;
- assertions that the edited document still parses through the normal
  validation path.

- [ ] CST-aware editing helper.
