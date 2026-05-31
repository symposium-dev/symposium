//! Verifies that JSON examples in `md/reference/hook-events.md` deserialize
//! as valid symposium hook events and round-trip cleanly through serde.

use symposium::hook_schema::symposium::{InputEvent, OutputEvent};

fn extract_json_blocks(markdown: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current = String::new();
    let mut block_start_line = 0;

    for (line_no, line) in markdown.lines().enumerate() {
        if !in_block {
            if line.trim_start().starts_with("```json") {
                in_block = true;
                current.clear();
                block_start_line = line_no + 1;
            }
        } else if line.trim_start().starts_with("```") {
            blocks.push((block_start_line, current.clone()));
            in_block = false;
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }

    blocks
}

/// Determine if a JSON block is under an "Input" or "Output" top-level section
/// by scanning backwards from the block's line number for the nearest `## `
/// (level-2) heading.
fn parent_section_for_line(markdown: &str, target_line: usize) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    for i in (0..target_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
            return trimmed.to_lowercase();
        }
    }
    String::new()
}

fn is_input_section(section: &str) -> bool {
    section.contains("input")
}

fn is_output_section(section: &str) -> bool {
    section.contains("output")
}

#[test]
fn doc_json_examples_roundtrip() {
    let doc = include_str!("../md/reference/hook-events.md");
    let blocks = extract_json_blocks(doc);

    assert!(
        blocks.len() >= 8,
        "expected at least 8 JSON blocks (4 input + 4 output), found {}",
        blocks.len()
    );

    let mut input_count = 0;
    let mut output_count = 0;

    for (line_no, json_str) in &blocks {
        let section = parent_section_for_line(doc, *line_no);

        // Skip blocks in the "Testing" section — those are CLI examples
        if section.contains("testing") {
            continue;
        }

        if is_input_section(&section) {
            let parsed: InputEvent = serde_json::from_str(json_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse InputEvent from doc (line {}): {e}\nJSON: {json_str}",
                    line_no
                )
            });
            let reserialized = serde_json::to_value(&parsed).unwrap();
            let original: serde_json::Value = serde_json::from_str(json_str).unwrap();
            assert_eq!(
                reserialized, original,
                "InputEvent round-trip mismatch at line {line_no}"
            );
            input_count += 1;
        } else if is_output_section(&section) {
            let parsed: OutputEvent = serde_json::from_str(json_str).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse OutputEvent from doc (line {}): {e}\nJSON: {json_str}",
                    line_no
                )
            });
            let reserialized = serde_json::to_value(&parsed).unwrap();
            let original: serde_json::Value = serde_json::from_str(json_str).unwrap();
            assert_eq!(
                reserialized, original,
                "OutputEvent round-trip mismatch at line {line_no}"
            );
            output_count += 1;
        } else {
            panic!(
                "JSON block at line {line_no} is not under a recognized input/output section: {section:?}"
            );
        }
    }

    assert!(
        input_count >= 4,
        "expected at least 4 input examples, found {input_count}"
    );
    assert!(
        output_count >= 4,
        "expected at least 4 output examples, found {output_count}"
    );
}
