//! JSON Schema to TypeScript type expressions.
//!
//! Backing MCP servers describe their tool parameters with JSON Schema, but the
//! agent writes JavaScript against those tools. Models read TypeScript
//! declarations far more reliably than raw JSON Schema, so schemas are rendered
//! as `.d.ts`-style type expressions.
//!
//! Two rules govern everything here:
//!
//! * **Never fail.** Schemas arrive from arbitrary third-party servers across
//!   several JSON Schema dialects. A construct we do not recognize renders as
//!   `unknown` and generation continues.
//! * **`unknown`, not `any`.** `unknown` forces the model to narrow the value
//!   rather than silently assuming a shape that may not hold.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Rendered when a schema is missing, unrecognized, or unconstrained.
const UNKNOWN: &str = "unknown";

/// Deepest nesting we will follow before giving up.
///
/// Schemas come from third parties and may nest arbitrarily; this keeps a
/// pathological one from exhausting the stack.
const MAX_DEPTH: usize = 64;

/// Total subschemas we will render for one tool.
///
/// Composition keywords can fan out multiplicatively, so a depth cap alone is
/// not enough to bound the work.
const MAX_NODES: usize = 10_000;

/// Renders schemas to TypeScript, accumulating the named types they reference.
///
/// One renderer is shared across all of a server's tools so a type defined in
/// several tools' `$defs` is emitted once.
#[derive(Debug, Default)]
pub struct TypeRenderer {
    /// Type name to rendered body, ordered so output is stable.
    named: BTreeMap<String, String>,
}

impl TypeRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render `schema` as a type expression, registering any named types it
    /// references. `$ref` pointers resolve against `schema` itself.
    pub fn render(&mut self, schema: &Value) -> String {
        self.render_indented(schema, 0)
    }

    /// Render at a given nesting level, so a type embedded inside another
    /// block lines up with it.
    pub fn render_indented(&mut self, schema: &Value, indent: usize) -> String {
        let mut cx = Cx {
            root: schema,
            named: &mut self.named,
            in_progress: Vec::new(),
            budget: MAX_NODES,
        };
        render_at(&mut cx, schema, indent, 0)
    }

    /// The named type declarations collected so far, in name order.
    ///
    /// Always an alias: a body may be a union or intersection of objects (a
    /// serde externally-tagged enum, say), and `interface` accepts only a
    /// single object literal.
    pub fn declarations(&self) -> String {
        let mut out = String::new();
        for (name, body) in &self.named {
            out.push_str(&format!("type {name} = {body};\n\n"));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.named.is_empty()
    }
}

/// Per-render state: the document `$ref` resolves against, the shared type
/// table, the names currently being rendered, and the remaining node budget.
struct Cx<'a> {
    root: &'a Value,
    named: &'a mut BTreeMap<String, String>,
    in_progress: Vec<String>,
    budget: usize,
}

fn render_at(cx: &mut Cx, schema: &Value, indent: usize, depth: usize) -> String {
    if depth > MAX_DEPTH || cx.budget == 0 {
        return UNKNOWN.to_string();
    }
    cx.budget -= 1;

    match schema {
        // A schema may be a bare boolean: `true` accepts anything, `false`
        // accepts nothing.
        Value::Bool(true) => UNKNOWN.to_string(),
        Value::Bool(false) => "never".to_string(),
        Value::Object(map) => render_object_schema(cx, map, indent, depth),
        // Anything else is malformed; degrade rather than fail.
        _ => UNKNOWN.to_string(),
    }
}

fn render_object_schema(
    cx: &mut Cx,
    schema: &Map<String, Value>,
    indent: usize,
    depth: usize,
) -> String {
    if schema.is_empty() {
        return UNKNOWN.to_string();
    }

    // A `$ref` wins over every sibling keyword. Note that siblings do appear
    // alongside it: one deployed generator emits `description` next to `$ref`,
    // and a resolver that returned the target directly would drop it. The
    // description is read from the referring schema by the caller, so it
    // survives.
    if let Some(Value::String(pointer)) = schema.get("$ref") {
        return render_ref(cx, pointer, depth);
    }

    let base = render_constrained(cx, schema, indent, depth);

    // `nullable` is an OpenAPI 3.0 extension rather than JSON Schema, but the
    // schemars 0.8 line emits it for optional fields and those servers are
    // deployed today.
    if schema.get("nullable") == Some(&Value::Bool(true)) && base != "null" {
        return join_union(vec![base, "null".to_string()]);
    }
    base
}

/// Resolve a `$ref`, emitting the target as a named type and returning its
/// name.
///
/// Naming the type is what makes recursive schemas work: a self-reference
/// resolves to a name that is already being defined, rather than expanding
/// forever.
fn render_ref(cx: &mut Cx, pointer: &str, depth: usize) -> String {
    let Some(name) = type_name_for(pointer) else {
        // A pointer we cannot name (an external URL, say) is unresolvable.
        return UNKNOWN.to_string();
    };

    // Already emitted, or currently being emitted higher up the stack. The
    // latter is the cycle case; returning the name closes the loop.
    if cx.named.contains_key(&name) || cx.in_progress.contains(&name) {
        return name;
    }

    let Some(target) = resolve_pointer(cx.root, pointer) else {
        return UNKNOWN.to_string();
    };
    // Reserve the name before rendering the body, so a self-reference
    // encountered inside sees it as in progress.
    cx.in_progress.push(name.clone());
    let body = render_at(cx, &target.clone(), 0, depth + 1);
    cx.in_progress.pop();

    // `type A = A` is circular (TS2456); `{"$ref": "#"}` produces it.
    let body = if body == name {
        UNKNOWN.to_string()
    } else {
        body
    };

    cx.named.insert(name.clone(), body);
    name
}

/// Follow a JSON pointer such as `#/$defs/Message` within `root`.
fn resolve_pointer<'a>(root: &'a Value, pointer: &str) -> Option<&'a Value> {
    let path = pointer.strip_prefix('#')?;
    if path.is_empty() || path == "/" {
        return Some(root);
    }
    let mut current = root;
    for raw in path.trim_start_matches('/').split('/') {
        // Per RFC 6901, `~1` is an escaped `/` and `~0` an escaped `~`.
        let key = raw.replace("~1", "/").replace("~0", "~");
        current = current.get(&key)?;
    }
    Some(current)
}

/// Derive a TypeScript type name from a pointer's last segment.
fn type_name_for(pointer: &str) -> Option<String> {
    if !pointer.starts_with('#') {
        return None;
    }
    let last = pointer.rsplit('/').next()?;
    let cleaned: String = last
        .replace("~1", "/")
        .replace("~0", "~")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    // A type name may not start with a digit, nor be a reserved word.
    if cleaned.starts_with(|c: char| c.is_ascii_digit()) || is_reserved_type_name(&cleaned) {
        Some(format!("_{cleaned}"))
    } else {
        Some(cleaned)
    }
}

/// Names TypeScript will not accept as a declared type. `$defs` keys are the
/// server's own field names, so `default` and `string` are both plausible.
fn is_reserved_type_name(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        // Built-in and intrinsic types.
        "any",
        "bigint",
        "boolean",
        "never",
        "null",
        "number",
        "object",
        "string",
        "symbol",
        "undefined",
        "unknown",
        "void",
        // Keywords that cannot begin a declaration name.
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
    ];
    RESERVED.contains(&name)
}

/// Dispatch on whichever constraint keyword the schema uses, narrowest first.
fn render_constrained(
    cx: &mut Cx,
    schema: &Map<String, Value>,
    indent: usize,
    depth: usize,
) -> String {
    // A single permitted value is narrower than anything else present.
    if let Some(value) = schema.get("const") {
        return render_literal(value);
    }

    // `enum` constrains the value regardless of any declared `type`.
    if let Some(Value::Array(values)) = schema.get("enum") {
        return render_enum(values);
    }

    // Composition keywords. `allOf` combines constraints; `anyOf`/`oneOf`
    // offer alternatives. A single-element `allOf` — the shape generators emit
    // to attach a description to a reference — collapses to its one member.
    if let Some(Value::Array(members)) = schema.get("allOf") {
        return render_intersection(cx, members, indent, depth);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(members)) = schema.get(key) {
            return render_union(cx, members, indent, depth);
        }
    }

    match schema.get("type") {
        Some(Value::String(ty)) => render_with_type(cx, schema, ty, indent, depth),
        // A type may be a list of alternatives, e.g. `["string", "null"]`.
        Some(Value::Array(types)) => {
            let parts = types
                .iter()
                .filter_map(Value::as_str)
                .map(|ty| render_with_type(cx, schema, ty, indent, depth))
                .collect();
            join_union(parts)
        }
        _ => UNKNOWN.to_string(),
    }
}

fn render_with_type(
    cx: &mut Cx,
    schema: &Map<String, Value>,
    ty: &str,
    indent: usize,
    depth: usize,
) -> String {
    match ty {
        "string" => "string".to_string(),
        // JSON Schema separates integers from other numbers; TypeScript does not.
        "number" | "integer" => "number".to_string(),
        "boolean" => "boolean".to_string(),
        "null" => "null".to_string(),
        "array" => render_array(cx, schema, indent, depth),
        "object" => render_struct(cx, schema, indent, depth),
        _ => UNKNOWN.to_string(),
    }
}

fn render_array(cx: &mut Cx, schema: &Map<String, Value>, indent: usize, depth: usize) -> String {
    let Some(items) = schema.get("items") else {
        return format!("{UNKNOWN}[]");
    };
    // `A | B[]` would parse as `A | (B[])`, so a union element needs parens.
    let inner = parenthesize_if_composite(render_at(cx, items, indent, depth + 1));
    format!("{inner}[]")
}

fn render_struct(cx: &mut Cx, schema: &Map<String, Value>, indent: usize, depth: usize) -> String {
    let Some(Value::Object(properties)) = schema.get("properties") else {
        // No declared properties: the schema describes a map, and
        // `additionalProperties` types its values.
        //
        // `propertyNames` is deliberately ignored — it constrains keys, which
        // TypeScript index signatures cannot express.
        return match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => "{}".to_string(),
            Some(value @ Value::Object(_)) => {
                format!(
                    "Record<string, {}>",
                    render_at(cx, value, indent, depth + 1)
                )
            }
            _ => format!("Record<string, {UNKNOWN}>"),
        };
    };

    if properties.is_empty() {
        return "{}".to_string();
    }

    let required: Vec<&str> = match schema.get("required") {
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };

    let pad = "  ".repeat(indent + 1);
    let close_pad = "  ".repeat(indent);
    let mut out = String::from("{\n");

    for (name, subschema) in properties {
        if let Some(doc) = doc_comment(subschema) {
            out.push_str(&format!("{pad}/** {doc} */\n"));
        }
        let optional = if required.contains(&name.as_str()) {
            ""
        } else {
            "?"
        };
        let rendered = render_at(cx, subschema, indent + 1, depth + 1);
        out.push_str(&format!(
            "{pad}{}{optional}: {rendered};\n",
            property_key(name)
        ));
    }

    out.push_str(&close_pad);
    out.push('}');
    out
}

fn render_union(cx: &mut Cx, members: &[Value], indent: usize, depth: usize) -> String {
    let mut parts = Vec::new();
    for member in members {
        let rendered = render_at(cx, member, indent, depth + 1);
        if !is_direct_self_reference(cx, &rendered) {
            parts.push(rendered);
        }
    }
    join_union(parts)
}

fn render_intersection(cx: &mut Cx, members: &[Value], indent: usize, depth: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    for member in members {
        let rendered = render_at(cx, member, indent, depth + 1);
        if is_direct_self_reference(cx, &rendered) {
            continue;
        }
        let rendered = parenthesize_if_composite(rendered);
        if !parts.contains(&rendered) {
            parts.push(rendered);
        }
    }
    match parts.len() {
        0 => UNKNOWN.to_string(),
        1 => parts.pop().unwrap_or_default(),
        _ => parts.join(" & "),
    }
}

/// Whether a rendered member is the type currently being defined.
///
/// `type A = A | string` is circular (TS2456), and a member that *is* the
/// whole type constrains nothing. Only a bare name counts: `type Json =
/// string | Json[]` is legitimate recursion.
fn is_direct_self_reference(cx: &Cx, rendered: &str) -> bool {
    cx.in_progress.iter().any(|name| name == rendered)
}

fn render_enum(values: &[Value]) -> String {
    if values.is_empty() {
        return "never".to_string();
    }
    join_union(values.iter().map(render_literal).collect())
}

/// Join alternatives into a union, dropping duplicates and flattening the
/// `unknown` case — `T | unknown` is just `unknown`.
fn join_union(parts: Vec<String>) -> String {
    let mut unique: Vec<String> = Vec::new();
    for part in parts {
        if part == UNKNOWN {
            return UNKNOWN.to_string();
        }
        if !unique.contains(&part) {
            unique.push(part);
        }
    }
    match unique.len() {
        0 => UNKNOWN.to_string(),
        1 => unique.pop().unwrap_or_default(),
        _ => unique.join(" | "),
    }
}

/// Parenthesize a composite so it binds correctly inside a larger type.
///
/// `A | B[]` parses as `A | (B[])`, and `A & B[]` as `A & (B[])`.
fn parenthesize_if_composite(rendered: String) -> String {
    let composite = rendered.contains(" | ") || rendered.contains(" & ");
    if composite && !rendered.starts_with('(') {
        format!("({rendered})")
    } else {
        rendered
    }
}

/// Render a JSON value as a TypeScript literal type.
fn render_literal(value: &Value) -> String {
    match value {
        Value::String(s) => format!("{s:?}"),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        // An object or array literal has no TypeScript literal-type spelling.
        _ => UNKNOWN.to_string(),
    }
}

/// A property's `description`, collapsed to one line and safe inside a JSDoc
/// block.
fn doc_comment(schema: &Value) -> Option<String> {
    jsdoc_text(schema.get("description")?.as_str()?)
}

/// Collapse text to one line and make it safe inside a JSDoc block.
pub(crate) fn jsdoc_text(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // `*/` inside a JSDoc comment would end it early.
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(collapsed.replace("*/", "* /"))
}

/// Quote a property name unless it is a valid JavaScript identifier.
///
/// Tool schemas routinely use names that are not identifiers (`content-type`,
/// `2fa`), and those are legal as quoted property keys.
fn property_key(name: &str) -> String {
    if is_js_identifier(name) {
        name.to_string()
    } else {
        format!("{name:?}")
    }
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

    fn render(schema: Value) -> String {
        TypeRenderer::new().render(&schema)
    }

    // -- scalars --

    #[test]
    fn renders_scalar_types() {
        assert_eq!(render(json!({"type": "string"})), "string");
        assert_eq!(render(json!({"type": "boolean"})), "boolean");
        assert_eq!(render(json!({"type": "null"})), "null");
    }

    /// JSON Schema distinguishes integers from other numbers; TypeScript has
    /// only `number`.
    #[test]
    fn renders_integer_as_number() {
        assert_eq!(render(json!({"type": "number"})), "number");
        assert_eq!(render(json!({"type": "integer"})), "number");
    }

    // -- degradation --

    #[test]
    fn renders_unconstrained_schemas_as_unknown() {
        assert_eq!(render(json!({})), "unknown");
        assert_eq!(render(json!(true)), "unknown");
        assert_eq!(render(json!({"type": "not-a-type"})), "unknown");
        assert_eq!(render(json!({"minimum": 3})), "unknown");
    }

    #[test]
    fn renders_uninhabited_schemas_as_never() {
        assert_eq!(render(json!(false)), "never");
        assert_eq!(render(json!({"enum": []})), "never");
    }

    // -- arrays --

    #[test]
    fn renders_arrays() {
        assert_eq!(
            render(json!({"type": "array", "items": {"type": "string"}})),
            "string[]"
        );
        assert_eq!(render(json!({"type": "array"})), "unknown[]");
    }

    /// `A | B[]` parses as `A | (B[])`, so a union element must be parenthesized.
    #[test]
    fn parenthesizes_union_array_elements() {
        let out = render(json!({
            "type": "array",
            "items": {"enum": ["a", "b"]},
        }));
        assert_eq!(out, r#"("a" | "b")[]"#);
    }

    /// The same hazard one keyword over: `A & B[]` parses as `A & (B[])`.
    /// Unlike the union case this stayed valid syntax, so nothing downstream
    /// reported it — the type was simply wrong.
    #[test]
    fn parenthesizes_intersection_array_elements() {
        let out = render(json!({
            "type": "array",
            "items": {
                "allOf": [
                    {"type": "object", "properties": {"a": {"type": "string"}}},
                    {"type": "object", "properties": {"b": {"type": "number"}}},
                ],
            },
        }));
        assert!(out.starts_with('('), "should be parenthesized, got: {out}");
        assert!(out.ends_with(")[]"), "got: {out}");
    }

    // -- enums --

    #[test]
    fn renders_string_enum_as_literal_union() {
        assert_eq!(render(json!({"enum": ["a", "b"]})), r#""a" | "b""#);
    }

    /// Enum members are not always strings, and a declared `type` must not
    /// override the narrower `enum` constraint.
    #[test]
    fn renders_mixed_enum_and_ignores_declared_type() {
        assert_eq!(render(json!({"enum": [1, true, null]})), "1 | true | null");
        assert_eq!(
            render(json!({"type": "boolean", "enum": [true]})),
            "true",
            "enum should win over type"
        );
    }

    // -- references and named types --

    /// Render a schema and return both the expression and the declarations of
    /// every named type it pulled in.
    fn render_with_defs(schema: Value) -> (String, String) {
        let mut r = TypeRenderer::new();
        let expr = r.render(&schema);
        (expr, r.declarations())
    }

    #[test]
    fn resolves_ref_into_a_named_type() {
        let (expr, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {"msg": {"$ref": "#/$defs/Message"}},
            "required": ["msg"],
            "$defs": {
                "Message": {
                    "type": "object",
                    "properties": {"text": {"type": "string"}},
                    "required": ["text"],
                },
            },
        }));
        assert_eq!(expr, "{\n  msg: Message;\n}");
        assert_eq!(defs, "type Message = {\n  text: string;\n};\n\n");
    }

    /// A serde externally-tagged enum — the most common schemars-generated
    /// `$def`. Its body is a union of objects, so it starts with `{` without
    /// being an object literal; declaring it as an `interface` produced
    /// `interface Shape { … } | { … }`, a syntax error that took the whole
    /// declaration file down with it.
    #[test]
    fn a_union_bodied_named_type_is_declared_as_an_alias() {
        let (_, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {"shape": {"$ref": "#/$defs/Shape"}},
            "$defs": {
                "Shape": {
                    "oneOf": [
                        {"type": "object", "properties": {"Circle": {"type": "number"}},
                         "required": ["Circle"]},
                        {"type": "object", "properties": {"Square": {"type": "number"}},
                         "required": ["Square"]},
                    ],
                },
            },
        }));
        assert!(defs.starts_with("type Shape = {"), "got:\n{defs}");
        assert!(defs.contains("} | {"), "got:\n{defs}");
        assert!(!defs.contains("interface"), "got:\n{defs}");
    }

    /// Same shape, one keyword over: an intersection body.
    #[test]
    fn an_intersection_bodied_named_type_is_declared_as_an_alias() {
        let (_, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {"merged": {"$ref": "#/$defs/Merged"}},
            "$defs": {
                "Merged": {
                    "allOf": [
                        {"type": "object", "properties": {"a": {"type": "string"}}},
                        {"type": "object", "properties": {"b": {"type": "number"}}},
                    ],
                },
            },
        }));
        assert!(defs.starts_with("type Merged = {"), "got:\n{defs}");
        assert!(defs.contains("} & {"), "got:\n{defs}");
    }

    /// `type A = A | string` is a circular alias TypeScript rejects (TS2456).
    /// A member that is the whole type constrains nothing, so it drops out.
    #[test]
    fn a_direct_self_reference_does_not_produce_a_circular_alias() {
        let (_, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {"a": {"$ref": "#/$defs/A"}},
            "$defs": {
                "A": {"anyOf": [{"$ref": "#/$defs/A"}, {"type": "string"}]},
            },
        }));
        assert_eq!(defs, "type A = string;\n\n", "got:\n{defs}");
    }

    /// Recursion through a constructor is legitimate and must survive: the
    /// reference sits inside an array rather than being the alternative.
    #[test]
    fn recursion_through_an_array_is_kept() {
        let (_, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {"tree": {"$ref": "#/$defs/Json"}},
            "$defs": {
                "Json": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"$ref": "#/$defs/Json"}},
                    ],
                },
            },
        }));
        assert_eq!(defs, "type Json = string | Json[];\n\n", "got:\n{defs}");
    }

    /// A pointer to the whole document refers to itself and can only be
    /// `unknown`.
    #[test]
    fn a_root_self_reference_degrades() {
        let (_, defs) = render_with_defs(json!({"$ref": "#"}));
        assert!(!defs.contains("_ = _"), "circular alias, got:\n{defs}");
    }

    /// `$defs` keys are the server's own field names, so a reserved word is
    /// plausible. `type default =` is a parse error and `type string =` is a
    /// redeclaration.
    #[test]
    fn reserved_words_are_not_used_as_type_names() {
        for word in ["default", "string", "function", "enum"] {
            let (_, defs) = render_with_defs(json!({
                "type": "object",
                "properties": {"v": {"$ref": format!("#/$defs/{word}")}},
                "$defs": {word: {"type": "string"}},
            }));
            assert!(
                defs.starts_with(&format!("type _{word} =")),
                "`{word}` should be prefixed, got:\n{defs}"
            );
        }
    }

    /// The older dialect spells the same thing `definitions`.
    #[test]
    fn resolves_legacy_definitions_pointer() {
        let (expr, defs) = render_with_defs(json!({
            "$ref": "#/definitions/Role",
            "definitions": {"Role": {"enum": ["admin", "user"]}},
        }));
        assert_eq!(expr, "Role");
        assert_eq!(defs, "type Role = \"admin\" | \"user\";\n\n");
    }

    /// Naming the target is what makes recursion terminate: the self-reference
    /// resolves to a name that is already being defined.
    #[test]
    fn recursive_ref_terminates() {
        let (expr, defs) = render_with_defs(json!({
            "$ref": "#/$defs/Node",
            "$defs": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "children": {"type": "array", "items": {"$ref": "#/$defs/Node"}},
                    },
                },
            },
        }));
        assert_eq!(expr, "Node");
        assert!(
            defs.contains("children?: Node[];"),
            "self-reference should render as the name, got:\n{defs}"
        );
    }

    /// Two schemas referring to each other must not expand forever.
    #[test]
    fn mutually_recursive_refs_terminate() {
        let (_, defs) = render_with_defs(json!({
            "$ref": "#/$defs/A",
            "$defs": {
                "A": {"type": "object", "properties": {"b": {"$ref": "#/$defs/B"}}},
                "B": {"type": "object", "properties": {"a": {"$ref": "#/$defs/A"}}},
            },
        }));
        assert!(defs.contains("type A ="), "got:\n{defs}");
        assert!(defs.contains("type B ="), "got:\n{defs}");
    }

    /// One deployed generator emits `description` beside `$ref`. Resolving the
    /// reference must not discard it.
    #[test]
    fn ref_with_sibling_description_keeps_the_description() {
        let (expr, _) = render_with_defs(json!({
            "type": "object",
            "properties": {
                "msg": {
                    "$ref": "#/$defs/Message",
                    "description": "the message",
                },
            },
            "$defs": {"Message": {"type": "string"}},
        }));
        assert!(expr.contains("/** the message */"), "got:\n{expr}");
        assert!(expr.contains("msg?: Message;"), "got:\n{expr}");
    }

    /// A type shared by two properties is emitted once.
    #[test]
    fn shared_ref_is_emitted_once() {
        let (_, defs) = render_with_defs(json!({
            "type": "object",
            "properties": {
                "from": {"$ref": "#/$defs/User"},
                "to": {"$ref": "#/$defs/User"},
            },
            "$defs": {"User": {"type": "object", "properties": {"id": {"type": "string"}}}},
        }));
        assert_eq!(defs.matches("type User =").count(), 1, "got:\n{defs}");
    }

    #[test]
    fn unresolvable_refs_degrade_to_unknown() {
        assert_eq!(render(json!({"$ref": "#/$defs/Missing"})), "unknown");
        assert_eq!(
            render(json!({"$ref": "https://example.com/schema.json"})),
            "unknown",
            "an external reference cannot be resolved offline"
        );
    }

    /// A renderer is shared across a server's tools, so a type defined in two
    /// tools' `$defs` is emitted once.
    #[test]
    fn named_types_accumulate_across_renders() {
        let mut r = TypeRenderer::new();
        for _ in 0..2 {
            r.render(&json!({
                "$ref": "#/$defs/Shared",
                "$defs": {"Shared": {"type": "string"}},
            }));
        }
        assert_eq!(r.declarations().matches("type Shared").count(), 1);
    }

    // -- traversal bounds --

    /// Schemas arrive from third parties; deep nesting must not exhaust the
    /// stack.
    #[test]
    fn deep_nesting_is_bounded() {
        let mut schema = json!({"type": "string"});
        for _ in 0..(MAX_DEPTH + 50) {
            schema = json!({"type": "array", "items": schema});
        }
        let out = render(schema);
        assert!(out.ends_with("[]"), "should still render, got: {out}");
        assert!(
            out.contains(UNKNOWN),
            "the bound should show up as unknown at the bottom, got: {out}"
        );
    }

    // -- unions and composition --

    /// The dominant Python generator emits this for every optional field, and
    /// it is the single most common construct the naive mapping would drop.
    #[test]
    fn renders_nullable_anyof_as_union() {
        let out = render(json!({
            "anyOf": [{"type": "string"}, {"type": "null"}],
            "default": null,
        }));
        assert_eq!(out, "string | null");
    }

    /// Optionality is decided by `required`. A nullable field that *is*
    /// required stays non-optional; only its type gains `null`.
    #[test]
    fn nullable_union_does_not_imply_optional() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "a": {"anyOf": [{"type": "string"}, {"type": "null"}]},
            },
            "required": ["a"],
        }));
        assert_eq!(out, "{\n  a: string | null;\n}");
    }

    #[test]
    fn renders_oneof_as_union() {
        let out = render(json!({
            "oneOf": [{"type": "string"}, {"type": "number"}],
        }));
        assert_eq!(out, "string | number");
    }

    #[test]
    fn renders_allof_as_intersection() {
        let out = render(json!({
            "allOf": [
                {"type": "object", "properties": {"a": {"type": "string"}}},
                {"type": "object", "properties": {"b": {"type": "number"}}},
            ],
        }));
        assert_eq!(out, "{\n  a?: string;\n} & {\n  b?: number;\n}");
    }

    /// Generators wrap a single member in `allOf` to attach a sibling
    /// description. That must collapse rather than render a one-sided `&`.
    #[test]
    fn collapses_single_member_allof() {
        let out = render(json!({
            "allOf": [{"type": "string"}],
            "description": "wrapped",
        }));
        assert_eq!(out, "string");
    }

    /// `T | unknown` is just `unknown`, and repeated alternatives collapse.
    #[test]
    fn union_absorbs_unknown_and_drops_duplicates() {
        assert_eq!(
            render(json!({"anyOf": [{"type": "string"}, {}]})),
            "unknown"
        );
        assert_eq!(
            render(json!({"anyOf": [{"type": "string"}, {"type": "string"}]})),
            "string"
        );
    }

    // -- type as a list --

    #[test]
    fn renders_array_valued_type_as_union() {
        assert_eq!(render(json!({"type": ["string", "null"]})), "string | null");
        assert_eq!(
            render(json!({"type": ["string", "number", "boolean"]})),
            "string | number | boolean"
        );
    }

    // -- const --

    #[test]
    fn renders_const_as_literal() {
        assert_eq!(render(json!({"const": "resource"})), r#""resource""#);
        assert_eq!(render(json!({"const": 3})), "3");
    }

    // -- nullable (OpenAPI spelling) --

    /// One deployed Rust generator marks optional fields with `nullable`
    /// rather than a union or a null-typed alternative.
    #[test]
    fn renders_openapi_nullable_as_union() {
        let out = render(json!({
            "type": "array",
            "items": {"type": "string"},
            "nullable": true,
        }));
        assert_eq!(out, "string[] | null");
    }

    #[test]
    fn nullable_null_type_does_not_repeat_null() {
        assert_eq!(render(json!({"type": "null", "nullable": true})), "null");
    }

    // -- records --

    #[test]
    fn renders_typed_additional_properties_as_record() {
        let out = render(json!({
            "type": "object",
            "additionalProperties": {"type": "string"},
        }));
        assert_eq!(out, "Record<string, string>");
    }

    /// `propertyNames` constrains keys, which a TypeScript index signature
    /// cannot express, so it is ignored rather than degrading the type.
    #[test]
    fn ignores_property_names_constraint() {
        let out = render(json!({
            "type": "object",
            "propertyNames": {"type": "string"},
            "additionalProperties": {"type": "string"},
        }));
        assert_eq!(out, "Record<string, string>");
    }

    #[test]
    fn renders_closed_empty_object_as_empty_type() {
        let out = render(json!({"type": "object", "additionalProperties": false}));
        assert_eq!(out, "{}");
    }

    // -- objects --

    #[test]
    fn renders_object_with_required_and_optional_properties() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "sql": {"type": "string"},
                "limit": {"type": "integer"},
            },
            "required": ["sql"],
        }));
        assert_eq!(out, "{\n  limit?: number;\n  sql: string;\n}");
    }

    /// Optionality comes from `required` alone. Nothing else marks a property
    /// optional.
    #[test]
    fn properties_are_optional_when_required_is_absent() {
        let out = render(json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
        }));
        assert_eq!(out, "{\n  a?: string;\n}");
    }

    /// `additionalProperties: false` is the single most common construct in
    /// real tool schemas. Degrading on it would turn every ordinary object into
    /// `unknown`.
    #[test]
    fn additional_properties_false_does_not_degrade_the_object() {
        let out = render(json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "required": ["a"],
            "additionalProperties": false,
        }));
        assert_eq!(out, "{\n  a: string;\n}");
    }

    #[test]
    fn renders_object_without_properties_as_open_map() {
        assert_eq!(render(json!({"type": "object"})), "Record<string, unknown>");
        assert_eq!(render(json!({"type": "object", "properties": {}})), "{}");
    }

    #[test]
    fn renders_nested_objects_with_indentation() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "outer": {
                    "type": "object",
                    "properties": {"inner": {"type": "string"}},
                    "required": ["inner"],
                },
            },
            "required": ["outer"],
        }));
        assert_eq!(
            out, "{\n  outer: {\n    inner: string;\n  };\n}",
            "nested braces should indent, got:\n{out}"
        );
    }

    // -- property names --

    /// Tool schemas use property names that are not JavaScript identifiers.
    #[test]
    fn quotes_property_names_that_are_not_identifiers() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "content-type": {"type": "string"},
                "2fa": {"type": "boolean"},
                "ok_name": {"type": "string"},
            },
        }));
        assert!(out.contains(r#""content-type"?: string;"#), "got:\n{out}");
        assert!(out.contains(r#""2fa"?: boolean;"#), "got:\n{out}");
        assert!(
            out.contains("ok_name?: string;"),
            "valid identifiers stay unquoted, got:\n{out}"
        );
    }

    // -- documentation --

    #[test]
    fn renders_description_as_jsdoc() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "sql": {"type": "string", "description": "The query to run"},
            },
        }));
        assert_eq!(out, "{\n  /** The query to run */\n  sql?: string;\n}");
    }

    /// A description containing `*/` would close the JSDoc block early.
    #[test]
    fn escapes_comment_terminator_in_description() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "a": {"type": "string", "description": "ends the block */ here"},
            },
        }));
        assert!(!out.contains("*/ here"), "got:\n{out}");
        assert!(out.contains("* / here"), "got:\n{out}");
    }

    #[test]
    fn collapses_multiline_descriptions() {
        let out = render(json!({
            "type": "object",
            "properties": {
                "a": {"type": "string", "description": "first line\n  second line"},
            },
        }));
        assert!(out.contains("/** first line second line */"), "got:\n{out}");
    }
}
