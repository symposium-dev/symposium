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

/// Render one server's tools as a declaration block.
pub fn render_server(server: &str, tools: &[ToolDecl]) -> String {
    let mut types = TypeRenderer::new();
    let mut methods = String::new();

    for binding in binding_table(tools.iter().map(|t| t.name)) {
        let Some(tool) = tools.iter().find(|t| t.name == binding.wire_name) else {
            continue;
        };
        let params = render_params(&mut types, tool.input_schema);

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
            methods.push_str(&format!(
                "  {}({params}): Promise<unknown>;\n",
                render_key(key)
            ));
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

    /// Return types are deliberately untyped: almost no server in the wild
    /// declares an output schema, so promising a shape would be a lie.
    #[test]
    fn every_tool_returns_a_promise_of_unknown() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let out = render_server("s", &[tool("t", &schema)]);
        assert!(out.contains("): Promise<unknown>;"), "got:\n{out}");
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
}
