//! Native delivery names derived from trusted task inputs.
//! Tasks require distinct normalized names: predicate filtering makes collision
//! suffixes depend on runtime applicability.

pub(super) fn normalized_name(name: &str) -> Option<String> {
    let mut normalized = String::new();
    for character in name.chars().map(|character| character.to_ascii_lowercase()) {
        if character.is_ascii_alphanumeric() {
            normalized.push(character);
        } else if !normalized.is_empty() && !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    normalized.truncate(64);
    let normalized = normalized.trim_end_matches('-');
    (!normalized.is_empty()).then(|| normalized.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_names_are_portable_and_bounded() {
        assert_eq!(
            normalized_name("__My.Specialized_Plugin--"),
            Some("my-specialized-plugin".to_owned())
        );
        assert_eq!(normalized_name("--"), None);
        assert_eq!(normalized_name(&"a".repeat(100)).unwrap(), "a".repeat(64));
    }
}

#[cfg(test)]
mod applicability_tests;
