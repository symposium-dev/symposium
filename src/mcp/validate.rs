//! Checking a tool's result against the shape it declared.
//!
//! A tool's `outputSchema` becomes its TypeScript return type, but nothing in
//! MCP obliges a server to honor it, and answering with plain text is ordinary
//! rather than a defect. So the type is intent, not guarantee.
//!
//! **A mismatch never fails the call.** `"count: 3"` is not the declared object
//! but plainly carries the answer, and refusing it would throw that away. The
//! value passes through untouched and the model is told the shape did not hold —
//! the one thing it cannot work out for itself. Told nothing, it meets the
//! breach as an `undefined` property, blames its own code, and usually spends
//! another round trip probing the tool.
//!
//! **The notice is a tag, not prose.** A correction costing more context than
//! the type it corrects would defeat the design. Which field was wrong is left
//! out: the value travels with the tag, so the model can read what it got, and
//! the failure list goes to the log for whoever is debugging the server. That
//! is also why this is one validity check rather than an enumeration needing a
//! size cap.
//!
//! Two limits worth knowing:
//!
//! * **Remote `$ref`s are never resolved**, since fetching a URL a third-party
//!   schema names would turn a declaration into an outbound request. The
//!   dependency is built without its HTTP resolver, so such a reference simply
//!   makes the schema uncompilable.
//! * **An uncompilable schema is not checked at all**, matching the renderer's
//!   rule that generation never fails whatever a server sends. Nothing
//!   meaningful was declared, so nothing is owed.
//!
//! Schemas are compiled per call rather than cached: compilation is microseconds
//! against schemas this size, the call it guards just paid a subprocess round
//! trip, and a cache would need invalidating whenever a restarted server
//! re-advertised its tools.

use serde_json::Value;

/// Check `value` against `schema`, returning a tag for the model if it does not
/// conform.
///
/// `None` covers both "conforms" and "cannot be checked". Callers cannot
/// distinguish them, deliberately: neither is something to tell the model.
pub fn check_result(server: &str, tool: &str, schema: &Value, value: &Value) -> Option<String> {
    let validator = match jsonschema::validator_for(schema) {
        Ok(validator) => validator,
        Err(e) => {
            tracing::debug!(
                server,
                tool,
                error = %e,
                "output schema could not be compiled; result not checked"
            );
            return None;
        }
    };

    if validator.is_valid(value) {
        return None;
    }

    // Detail the tag deliberately omits, for whoever debugs the server.
    tracing::debug!(
        server,
        tool,
        failures = %validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; "),
        "result did not match the declared output schema"
    );

    Some(format!(
        "{server}.{tool}: result off-shape, treat as unknown"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {"count": {"type": "number"}},
            "required": ["count"],
        })
    }

    #[test]
    fn a_conforming_value_is_not_remarked_on() {
        assert_eq!(
            check_result("s", "t", &schema(), &json!({"count": 1})),
            None
        );
    }

    #[test]
    fn a_mismatch_names_the_tool_and_says_what_to_do() {
        let tag = check_result("memory", "search", &schema(), &json!({"count": "one"}))
            .expect("a string is not a number");
        assert_eq!(tag, "memory.search: result off-shape, treat as unknown");
    }

    #[test]
    fn a_missing_required_field_is_a_mismatch() {
        assert!(
            check_result("s", "t", &schema(), &json!({})).is_some(),
            "the required field is absent"
        );
    }

    /// The case that must never fail a call: the server answered with text, so
    /// the unwrap ladder handed back a string where an object was declared.
    /// Ordinary MCP, not a defect, so the value stands and the model is tagged.
    #[test]
    fn an_unstructured_answer_is_tagged_not_rejected() {
        let tag = check_result("s", "t", &schema(), &json!("count: 3"))
            .expect("a string is not the declared object");
        assert!(tag.contains("treat as unknown"), "got: {tag}");
    }

    /// Servers commonly put JSON in a text block. The unwrap ladder parses it,
    /// so it arrives as structured data and satisfies the schema.
    #[test]
    fn json_delivered_as_text_still_conforms() {
        let parsed: Value = serde_json::from_str(r#"{"count": 2}"#).unwrap();
        assert_eq!(check_result("s", "t", &schema(), &parsed), None);
    }

    /// The renderer never fails on a schema it cannot understand, so neither
    /// does the checker. Nothing meaningful was declared, so nothing is owed.
    #[test]
    fn an_uncompilable_schema_is_not_checked() {
        let bogus = json!({"$ref": "https://example.invalid/nope.json"});
        assert_eq!(
            check_result("s", "t", &bogus, &json!("anything")),
            None,
            "an unresolvable reference must not produce a tag"
        );
    }

    #[test]
    fn a_permissive_schema_accepts_anything() {
        let open = json!({"type": "object"});
        assert_eq!(
            check_result("s", "t", &open, &json!({"whatever": true})),
            None
        );
    }

    /// However wrong the value is, the tag is one short line. A result whose
    /// every element fails would otherwise crowd out the result it accompanies.
    #[test]
    fn the_tag_is_the_same_size_however_many_failures() {
        let strict = json!({"type": "array", "items": {"type": "number"}});
        let one_bad: Value = json!(["x"]);
        let all_bad: Value = (0..50).map(|_| json!("x")).collect();

        let first = check_result("s", "t", &strict, &one_bad).expect("one item is wrong");
        let second = check_result("s", "t", &strict, &all_bad).expect("every item is wrong");
        assert_eq!(
            first, second,
            "the tag must not grow with the failure count"
        );
        assert!(first.len() < 80, "tag should stay small: {}", first.len());
    }
}
