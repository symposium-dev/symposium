use regex::Regex;

const TEMPLATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/md/start.md"));

/// Render the start template, expanding `$INVOKE(arg1,arg2,...)` placeholders
/// into CLI-style invocations.
pub fn render() -> String {
    let re = Regex::new(r"\$INVOKE\(([^)]+)\)").unwrap();
    re.replace_all(TEMPLATE, |caps: &regex::Captures| {
        let args: Vec<&str> = caps[1].split(',').map(|s| s.trim()).collect();
        let joined = args.join(" ");
        format!("`symposium {joined}`")
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_expands_invoke() {
        let output = render();
        assert!(
            output.contains("`symposium crate $name`"),
            "should expand $INVOKE to symposium: {output}"
        );
        assert!(!output.contains("$INVOKE"));
    }
}
