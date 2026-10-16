fn json_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_block = false;
    for line in markdown.lines() {
        if !in_block {
            in_block = line.trim_start().starts_with("```json");
            current.clear();
        } else if line.trim_start().starts_with("```") {
            blocks.push(current.clone());
            in_block = false;
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    blocks
}

#[test]
fn reference_manifest_examples_load_as_packages() {
    let blocks = json_blocks(include_str!("../md/reference/agent-plugins.md"));
    assert_eq!(blocks.len(), 2, "{blocks:#?}");
    for json in blocks {
        let package = tempfile::tempdir().unwrap();
        std::fs::write(package.path().join("plugin.json"), &json).unwrap();
        let results = symposium::plugins::validate_source_dir(package.path()).unwrap();
        assert_eq!(results.len(), 1, "{json}");
        assert!(results[0].result.is_ok(), "{json}: {:?}", results[0].result);
        assert!(
            results[0].warnings.iter().all(|w| w.contains("dormant")),
            "{json}: {:?}",
            results[0].warnings
        );
    }
}
