use anyhow::{Context, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("md-orphan-check") => md_orphan_check(),
        Some(cmd) => bail!("unknown xtask command: {cmd}"),
        None => bail!(
            "usage: cargo xtask <command>\n\ncommands:\n  md-orphan-check    Check for .md files not referenced in SUMMARY.md"
        ),
    }
}

fn md_orphan_check() -> anyhow::Result<()> {
    let project_root = project_root();
    let md_dir = project_root.join("md");
    let summary_path = md_dir.join("SUMMARY.md");

    let summary_content =
        fs::read_to_string(&summary_path).context("failed to read md/SUMMARY.md")?;

    let referenced: BTreeSet<PathBuf> = summary_content
        .lines()
        .filter_map(|line| {
            let marker = line.find("(./")?;
            let start = marker + 1; // skip the '('
            let end = start + line[start..].find(')')?;
            let rel_path = &line[start..end];
            Some(md_dir.join(rel_path))
        })
        .map(|p| normalize_path(&p))
        .collect();

    let mut all_md_files: Vec<PathBuf> = Vec::new();
    walk_md_files(&md_dir, &mut all_md_files)?;
    all_md_files.sort();

    let mut orphans: Vec<PathBuf> = Vec::new();
    for file in &all_md_files {
        let normalized = normalize_path(file);
        if normalized == normalize_path(&summary_path) {
            continue;
        }
        if !referenced.contains(&normalized) {
            orphans.push(file.strip_prefix(&md_dir).unwrap_or(file).to_path_buf());
        }
    }

    if orphans.is_empty() {
        println!("No orphaned .md files found.");
        Ok(())
    } else {
        eprintln!(
            "Found {} orphaned .md file(s) not referenced in SUMMARY.md:\n",
            orphans.len()
        );
        for orphan in &orphans {
            eprintln!("  md/{}", orphan.display());
        }
        eprintln!("\nEither add them to md/SUMMARY.md or remove them.");
        bail!("orphan check failed");
    }
}

fn walk_md_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_md_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
    Ok(())
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in p.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop(); // up from xtask/
    dir
}
