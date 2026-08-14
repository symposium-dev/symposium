//! Rendering a backing server's tools as TypeScript declarations.
//!
//! Each server becomes one object of methods, so a script reads as
//! `await sqlx.query({ sql: "..." })`. The object form — rather than a
//! `declare namespace` — is what lets a tool whose wire name is not a
//! JavaScript identifier still be declared, since an object type accepts
//! quoted method names.

use serde_json::Value;

use super::schema_to_ts::{TypeRenderer, jsdoc_text};

/// One tool as advertised by a backing server.
#[derive(Debug, Clone, Copy)]
pub struct ToolDecl<'a> {
    /// The name used on the wire, which need not be a JavaScript identifier.
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub input_schema: Option<&'a Value>,
    /// The tool's declared output schema, when it has one. Rendered as the
    /// return type, and checked against the returned value at call time.
    pub output_schema: Option<&'a Value>,
}

/// The JavaScript keys one tool answers to, primary first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBinding {
    /// Name to send on the wire.
    pub wire_name: String,
    /// Property keys, as they appear on the namespace object. A wire name
    /// that is already an identifier has one; anything else also gets a
    /// sanitized alias, so `sqlx["migrate-status"]` and
    /// `sqlx.migrate_status` both dispatch.
    pub keys: Vec<String>,
}

/// Assign JavaScript keys to a server's tools.
///
/// The single source of both the declarations and the runtime bindings, so a
/// name the model is shown is one it can call.
///
/// Two passes: primaries before aliases, so a tool keeps its own spelling
/// rather than losing it to another tool's sanitized form.
pub fn binding_table<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<ToolBinding> {
    let mut used: Vec<String> = Vec::new();
    let mut table: Vec<ToolBinding> = Vec::new();

    for name in names {
        // Tool names are unique per server by the protocol; a repeat is
        // unaddressable on the wire either way.
        if table.iter().any(|b| b.wire_name == name) {
            continue;
        }
        table.push(ToolBinding {
            wire_name: name.to_string(),
            keys: vec![unique(name.to_string(), &mut used)],
        });
    }

    for binding in &mut table {
        if !is_js_identifier(&binding.wire_name) {
            binding
                .keys
                .push(unique(sanitize(&binding.wire_name), &mut used));
        }
    }

    table
}

/// A spelling collapsed for tolerant matching: case and punctuation dropped, so
/// `create_entities`, `createEntities` and `create-entities` agree.
fn normalized_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// What the property name a script used resolved to.
pub enum KeyMatch<'a> {
    One(&'a ToolBinding),
    /// Wire names of tools whose spellings collapse together.
    Ambiguous(Vec<&'a str>),
    None,
}

/// Resolve a script's property name to a tool.
///
/// Declared spellings match exactly first, so two tools differing only in
/// punctuation stay distinct. Everything else falls back to normalized
/// matching, which is what lets a model reach a snake_case tool by the
/// camelCase name TypeScript habit suggests. An ambiguous fallback is refused
/// rather than guessed.
pub fn resolve_key<'a>(table: &'a [ToolBinding], key: &str) -> KeyMatch<'a> {
    if let Some(binding) = table.iter().find(|b| b.keys.iter().any(|k| k == key)) {
        return KeyMatch::One(binding);
    }

    let wanted = normalized_key(key);
    if wanted.is_empty() {
        return KeyMatch::None;
    }

    let mut hits = table
        .iter()
        .filter(|b| b.keys.iter().any(|k| normalized_key(k) == wanted));

    match (hits.next(), hits.next()) {
        (Some(binding), None) => KeyMatch::One(binding),
        (Some(first), Some(second)) => {
            let mut names = vec![first.wire_name.as_str(), second.wire_name.as_str()];
            names.extend(hits.map(|b| b.wire_name.as_str()));
            KeyMatch::Ambiguous(names)
        }
        _ => KeyMatch::None,
    }
}

/// Render one server's tools as a declaration block.
pub fn render_server(server: &str, tools: &[ToolDecl]) -> String {
    let mut types = TypeRenderer::new();
    let mut methods = String::new();

    for binding in binding_table(tools.iter().map(|t| t.name)) {
        let Some(tool) = tools.iter().find(|t| t.name == binding.wire_name) else {
            continue;
        };
        let params = render_params(&mut types, tool.input_schema);
        let result = render_result(&mut types, tool.output_schema);

        for (index, key) in binding.keys.iter().enumerate() {
            if index == 0 {
                if let Some(doc) = tool.description.and_then(jsdoc_text) {
                    methods.push_str(&format!("  /** {doc} */\n"));
                }
            } else {
                // The alias is the same tool under a different spelling.
                // Repeating the description would double it in the output.
                let name = jsdoc_text(tool.name).unwrap_or_else(|| "the same tool".to_string());
                methods.push_str(&format!("  /** Alias for {name}. */\n"));
            }
            methods.push_str(&format!("  {}({params}): {result};\n", render_key(key)));
        }
    }

    let mut out = types.declarations();
    out.push_str(&format!(
        "declare const {}: {{\n{methods}}};\n",
        sanitize(server)
    ));
    out
}

/// A property key as it must be written in a type literal.
fn render_key(key: &str) -> String {
    if is_js_identifier(key) {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

/// Render a tool's parameter list.
///
/// A tool with no properties takes no argument at all, and one whose
/// properties are all optional takes an optional argument — both save the
/// model from passing an empty object.
fn render_params(types: &mut TypeRenderer, schema: Option<&Value>) -> String {
    let Some(schema) = schema else {
        return String::new();
    };
    if !has_properties(schema) {
        return String::new();
    }
    let optional = if has_required(schema) { "" } else { "?" };
    // Indent one level: the type sits inside the server object's braces.
    format!("params{optional}: {}", types.render_indented(schema, 1))
}

/// Render a tool's return type.
///
/// A declared output schema becomes the return type; a tool without one stays
/// `unknown`, which forces the model to narrow rather than assume a shape.
///
/// Nothing obliges a server to send what it declared, so this is intent rather
/// than guarantee. The same schema is checked against the value at call time
/// and a mismatch is tagged, not refused — see [`crate::mcp::validate`].
fn render_result(types: &mut TypeRenderer, schema: Option<&Value>) -> String {
    match schema {
        // Same indent as the parameter type: both sit inside the server
        // object's braces.
        Some(schema) => format!("Promise<{}>", types.render_indented(schema, 1)),
        None => "Promise<unknown>".to_string(),
    }
}

fn has_properties(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|p| !p.is_empty())
}

fn has_required(schema: &Value) -> bool {
    schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|r| !r.is_empty())
}

/// Coerce a wire name into a JavaScript identifier.
fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() {
        return "_".to_string();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Keep names distinct. Two wire names differing only in punctuation sanitize
/// to the same identifier; this is rare but must not silently drop a tool.
fn unique(name: String, used: &mut Vec<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while used.contains(&candidate) {
        candidate = format!("{name}_{n}");
        n += 1;
    }
    used.push(candidate.clone());
    candidate
}

fn is_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool<'a>(name: &'a str, schema: &'a Value) -> ToolDecl<'a> {
        ToolDecl {
            name,
            description: None,
            input_schema: Some(schema),
            output_schema: None,
        }
    }

    #[test]
    fn renders_a_tool_with_parameters() {
        let schema = json!({
            "type": "object",
            "properties": {"sql": {"type": "string"}},
            "required": ["sql"],
        });
        let out = render_server("sqlx", &[tool("query", &schema)]);
        assert_eq!(
            out,
            "declare const sqlx: {\n  query(params: {\n    sql: string;\n  }): Promise<unknown>;\n};\n"
        );
    }

    /// A tool that declares no output schema stays untyped. `unknown` forces
    /// the model to narrow rather than assume a shape nobody promised.
    #[test]
    fn a_tool_without_an_output_schema_returns_unknown() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let out = render_server("s", &[tool("t", &schema)]);
        assert!(out.contains("): Promise<unknown>;"), "got:\n{out}");
    }

    fn typed_tool<'a>(name: &'a str, input: &'a Value, output: &'a Value) -> ToolDecl<'a> {
        ToolDecl {
            name,
            description: None,
            input_schema: Some(input),
            output_schema: Some(output),
        }
    }

    #[test]
    fn a_declared_output_schema_becomes_the_return_type() {
        let input = json!({"type": "object", "properties": {"q": {"type": "string"}}});
        let output = json!({
            "type": "object",
            "properties": {"count": {"type": "integer"}},
            "required": ["count"],
        });
        let out = render_server("s", &[typed_tool("search", &input, &output)]);
        assert!(
            out.contains("): Promise<{\n    count: number;\n  }>;"),
            "got:\n{out}"
        );
        assert!(
            !out.contains("Promise<unknown>"),
            "a declared shape should replace `unknown`:\n{out}"
        );
    }

    /// A named type in an output schema goes through the same hoisting as one
    /// in a parameter schema, so it is declared once above the server object.
    #[test]
    fn a_named_type_in_an_output_schema_is_hoisted() {
        let input = json!({"type": "object"});
        let output = json!({
            "type": "object",
            "properties": {"hit": {"$ref": "#/definitions/Hit"}},
            "definitions": {
                "Hit": {
                    "title": "Hit",
                    "type": "object",
                    "properties": {"score": {"type": "number"}},
                },
            },
        });
        let out = render_server("s", &[typed_tool("find", &input, &output)]);
        let server_at = out.find("declare const s:").expect("server declared");
        let hit_at = out.find("Hit").expect("named type rendered");
        assert!(
            hit_at < server_at,
            "the named type should precede the server object:\n{out}"
        );
    }

    /// Both spellings of a hyphenated tool are the same tool, so both carry the
    /// same return type.
    #[test]
    fn an_aliased_tool_carries_its_return_type_on_both_spellings() {
        let input = json!({"type": "object"});
        let output = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
        let out = render_server("s", &[typed_tool("get-sum", &input, &output)]);
        let typed = out.matches("Promise<{").count();
        assert_eq!(typed, 2, "both spellings should be typed:\n{out}");
    }

    /// The renderer's never-fail rule applies to output schemas too: a
    /// construct it does not understand degrades rather than panicking.
    #[test]
    fn an_unrecognized_output_construct_degrades() {
        let input = json!({"type": "object"});
        let output = json!({"not-a-real-keyword": ["whatever"]});
        let out = render_server("s", &[typed_tool("t", &input, &output)]);
        assert!(out.contains("): Promise<"), "got:\n{out}");
    }

    /// A non-object output schema is still worth declaring.
    #[test]
    fn a_scalar_output_schema_is_declared() {
        let input = json!({"type": "object"});
        let output = json!({"type": "string"});
        let out = render_server("s", &[typed_tool("name", &input, &output)]);
        assert!(out.contains("): Promise<string>;"), "got:\n{out}");
    }

    /// A tool taking nothing should not force the model to pass `{}`.
    #[test]
    fn tool_without_properties_takes_no_argument() {
        let out = render_server("s", &[tool("ping", &json!({"type": "object"}))]);
        assert!(out.contains("ping(): Promise<unknown>;"), "got:\n{out}");
    }

    #[test]
    fn tool_with_only_optional_properties_takes_an_optional_argument() {
        let schema = json!({
            "type": "object",
            "properties": {"limit": {"type": "integer"}},
        });
        let out = render_server("s", &[tool("list", &schema)]);
        assert!(out.contains("list(params?: {"), "got:\n{out}");
    }

    #[test]
    fn missing_input_schema_takes_no_argument() {
        let out = render_server(
            "s",
            &[ToolDecl {
                name: "t",
                description: None,
                input_schema: None,
                output_schema: None,
            }],
        );
        assert!(out.contains("t(): Promise<unknown>;"), "got:\n{out}");
    }

    // -- naming --

    /// The protocol's own reference server names 12 of its 13 tools with
    /// hyphens, so this is the common case, not an edge case.
    #[test]
    fn hyphenated_tool_gets_both_spellings() {
        let out = render_server("s", &[tool("get-sum", &json!({}))]);
        assert!(
            out.contains(r#"  "get-sum"(): Promise<unknown>;"#),
            "the wire name must stay callable, got:\n{out}"
        );
        assert!(
            out.contains("  get_sum(): Promise<unknown>;"),
            "a dotted alias should also work, got:\n{out}"
        );
        assert!(out.contains("/** Alias for get-sum. */"), "got:\n{out}");
    }

    /// The alias carries only its cross-reference; repeating the description
    /// would print it twice for one tool.
    #[test]
    fn alias_does_not_repeat_the_description() {
        let out = render_server(
            "s",
            &[ToolDecl {
                name: "get-sum",
                description: Some("Adds numbers"),
                input_schema: None,
                output_schema: None,
            }],
        );
        assert_eq!(
            out.matches("Adds numbers").count(),
            1,
            "description should appear once, got:\n{out}"
        );
        assert!(out.contains("/** Alias for get-sum. */"), "got:\n{out}");
    }

    #[test]
    fn identifier_tool_is_not_aliased() {
        let out = render_server("s", &[tool("query", &json!({}))]);
        assert_eq!(out.matches("Promise<unknown>").count(), 1, "got:\n{out}");
        assert!(!out.contains('"'), "no quoting needed, got:\n{out}");
    }

    #[test]
    fn leading_digit_name_is_prefixed() {
        let out = render_server("s", &[tool("2fa", &json!({}))]);
        assert!(out.contains(r#""2fa"()"#), "got:\n{out}");
        assert!(out.contains("_2fa()"), "got:\n{out}");
    }

    /// Two wire names that sanitize to the same identifier must both survive.
    #[test]
    fn colliding_aliases_are_disambiguated() {
        let out = render_server(
            "s",
            &[tool("get-sum", &json!({})), tool("get.sum", &json!({}))],
        );
        assert!(out.contains("get_sum("), "got:\n{out}");
        assert!(out.contains("get_sum_2("), "got:\n{out}");
        assert!(out.contains(r#""get.sum"("#), "got:\n{out}");
    }

    #[test]
    fn server_name_is_sanitized() {
        let out = render_server("sea-orm", &[tool("t", &json!({}))]);
        assert!(out.starts_with("declare const sea_orm: {"), "got:\n{out}");
    }

    /// The method names in a rendered declaration block.
    fn declared_keys(out: &str) -> Vec<String> {
        out.lines()
            .filter_map(|line| line.trim().strip_suffix("): Promise<unknown>;"))
            .filter_map(|line| line.split('(').next())
            .map(|key| key.trim_matches('"').to_string())
            .collect()
    }

    #[test]
    fn an_identifier_name_needs_no_alias() {
        let table = binding_table(["query"]);
        assert_eq!(table[0].keys, vec!["query".to_string()]);
    }

    #[test]
    fn a_hyphenated_name_is_bound_under_both_spellings() {
        let table = binding_table(["get-sum"]);
        assert_eq!(
            table[0].keys,
            vec!["get-sum".to_string(), "get_sum".to_string()]
        );
    }

    /// `get-sum` sanitizes to `get_sum`, which another tool already owns.
    #[test]
    fn a_colliding_alias_never_shadows_a_real_tool() {
        let table = binding_table(["get-sum", "get_sum"]);

        assert_eq!(table[0].wire_name, "get-sum");
        assert_eq!(
            table[0].keys,
            vec!["get-sum".to_string(), "get_sum_2".to_string()],
            "the alias must yield to the tool that owns the name"
        );
        assert_eq!(table[1].wire_name, "get_sum");
        assert_eq!(
            table[1].keys,
            vec!["get_sum".to_string()],
            "a real tool keeps its own spelling"
        );

        let mut keys: Vec<&String> = table.iter().flat_map(|b| &b.keys).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "every key must be distinct: {keys:?}");
    }

    /// A name the model is shown must be one dispatch binds.
    #[test]
    fn declared_names_are_the_bound_names() {
        let schema = json!({});
        let tools = [
            tool("get-sum", &schema),
            tool("get_sum", &schema),
            tool("query", &schema),
        ];
        let out = render_server("s", &tools);

        let bound: Vec<String> = binding_table(tools.iter().map(|t| t.name))
            .into_iter()
            .flat_map(|b| b.keys)
            .collect();

        assert_eq!(declared_keys(&out), bound, "got:\n{out}");
    }

    /// Two members of the same name is a TypeScript error (TS2300).
    #[test]
    fn a_repeated_wire_name_is_declared_once() {
        let schema = json!({});
        let tools = [tool("get-sum", &schema), tool("get-sum", &schema)];
        let out = render_server("s", &tools);

        assert_eq!(binding_table(tools.iter().map(|t| t.name)).len(), 1);
        assert_eq!(
            declared_keys(&out),
            vec!["get-sum".to_string(), "get_sum".to_string()],
            "got:\n{out}"
        );
    }

    // -- documentation and shared types --

    #[test]
    fn renders_tool_description_as_jsdoc() {
        let out = render_server(
            "s",
            &[ToolDecl {
                name: "t",
                description: Some("Does  a\nthing"),
                input_schema: None,
                output_schema: None,
            }],
        );
        assert!(out.contains("/** Does a thing */"), "got:\n{out}");
    }

    /// Named types are hoisted above the object so several tools can share
    /// them.
    #[test]
    fn named_types_are_emitted_once_above_the_server() {
        let schema = json!({
            "type": "object",
            "properties": {"user": {"$ref": "#/$defs/User"}},
            "required": ["user"],
            "$defs": {"User": {"type": "object", "properties": {"id": {"type": "string"}}}},
        });
        let out = render_server("s", &[tool("a", &schema), tool("b", &schema)]);
        assert_eq!(out.matches("type User =").count(), 1, "got:\n{out}");
        assert!(
            out.find("type User =") < out.find("declare const s"),
            "types must precede the server object, got:\n{out}"
        );
    }

    // -- tolerant key resolution --

    fn table(names: &[&str]) -> Vec<ToolBinding> {
        binding_table(names.iter().copied())
    }

    fn resolved<'a>(table: &'a [ToolBinding], key: &str) -> &'a str {
        match resolve_key(table, key) {
            KeyMatch::One(binding) => binding.wire_name.as_str(),
            KeyMatch::Ambiguous(names) => panic!("ambiguous: {names:?}"),
            KeyMatch::None => panic!("`{key}` did not resolve"),
        }
    }

    #[test]
    fn a_declared_spelling_resolves_to_its_own_tool() {
        let t = table(&["create_entities", "get-sum"]);
        assert_eq!(resolved(&t, "create_entities"), "create_entities");
        assert_eq!(resolved(&t, "get-sum"), "get-sum");
        assert_eq!(resolved(&t, "get_sum"), "get-sum");
    }

    #[test]
    fn a_camel_case_key_reaches_a_snake_case_tool() {
        let t = table(&["create_entities"]);
        assert_eq!(resolved(&t, "createEntities"), "create_entities");
    }

    #[test]
    fn a_camel_case_key_reaches_a_kebab_case_tool() {
        let t = table(&["get-annotated-message"]);
        assert_eq!(resolved(&t, "getAnnotatedMessage"), "get-annotated-message");
    }

    #[test]
    fn punctuation_and_case_are_both_ignored() {
        let t = table(&["browser_console_messages"]);
        for key in [
            "browserConsoleMessages",
            "browser-console-messages",
            "BrowserConsoleMessages",
            "BROWSER_CONSOLE_MESSAGES",
        ] {
            assert_eq!(resolved(&t, key), "browser_console_messages", "key: {key}");
        }
    }

    /// A server exposing two spellings as separate tools must keep them apart,
    /// so an exact hit is never diverted by the tolerant fallback.
    #[test]
    fn an_exact_match_wins_over_a_normalized_one() {
        let t = table(&["read_file", "readFile"]);
        assert_eq!(resolved(&t, "read_file"), "read_file");
        assert_eq!(resolved(&t, "readFile"), "readFile");
    }

    /// With no exact hit, two tools that collapse together are refused rather
    /// than guessed between.
    #[test]
    fn colliding_spellings_are_refused() {
        let t = table(&["read_file", "readFile"]);
        match resolve_key(&t, "read-file") {
            KeyMatch::Ambiguous(names) => {
                assert!(names.contains(&"read_file"), "got {names:?}");
                assert!(names.contains(&"readFile"), "got {names:?}");
            }
            other => panic!(
                "expected ambiguity, got {}",
                match other {
                    KeyMatch::One(b) => format!("One({})", b.wire_name),
                    KeyMatch::None => "None".to_string(),
                    KeyMatch::Ambiguous(_) => unreachable!(),
                }
            ),
        }
    }

    #[test]
    fn an_unrelated_key_does_not_resolve() {
        let t = table(&["create_entities"]);
        assert!(matches!(resolve_key(&t, "deleteEntities"), KeyMatch::None));
    }

    /// Only what the table carries is reachable, so a tool filtered out of the
    /// visible set cannot be summoned by an alternate spelling.
    #[test]
    fn a_tool_absent_from_the_table_is_unreachable() {
        let t = table(&["read_file"]);
        assert!(matches!(resolve_key(&t, "writeFile"), KeyMatch::None));
        assert!(matches!(resolve_key(&t, "write_file"), KeyMatch::None));
    }

    #[test]
    fn a_key_with_no_alphanumerics_does_not_resolve() {
        let t = table(&["read_file"]);
        assert!(matches!(resolve_key(&t, "___"), KeyMatch::None));
        assert!(matches!(resolve_key(&t, ""), KeyMatch::None));
    }

    /// The sanitized alias of a hyphenated tool is a real declared key, so it
    /// resolves exactly rather than through the fallback.
    #[test]
    fn a_sanitized_alias_still_resolves_to_its_tool() {
        let t = table(&["migrate-status"]);
        assert_eq!(resolved(&t, "migrate_status"), "migrate-status");
        assert_eq!(resolved(&t, "migrateStatus"), "migrate-status");
    }
}
