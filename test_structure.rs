use std::path::Path;
use tempfile::TempDir;

// Simple test to verify our new discovery logic
fn main() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Create test structure
    std::fs::write(
        dir.join("root.toml"),
        r#"name = "root-plugin""#,
    ).unwrap();

    let skill_dir = dir.join("my-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: my-skill
crates: serde
---

Test skill."#,
    ).unwrap();

    let mixed_dir = dir.join("mixed");
    std::fs::create_dir_all(&mixed_dir).unwrap();
    std::fs::write(
        mixed_dir.join("SKILL.md"),
        r#"---
name: mixed-skill
crates: tokio
---

Mixed skill."#,
    ).unwrap();
    std::fs::write(
        mixed_dir.join("ignored.toml"),
        r#"name = "ignored""#,
    ).unwrap();

    println!("Test structure created at: {}", dir.display());
    println!("You can now test the discovery logic manually");
}