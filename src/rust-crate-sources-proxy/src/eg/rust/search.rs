//! Text searching within extracted crates

use crate::eg::{Result, EgError, Match};
use regex::Regex;
use std::fs;
use std::path::Path;

/// Handles text searching within extracted crate sources
pub struct CrateSearcher;

impl CrateSearcher {
    pub fn new() -> Self {
        Self
    }

    /// Search for pattern in the extracted crate, returning categorized matches
    pub fn search_crate(
        &self,
        crate_path: &Path,
        pattern: &Regex,
        context_lines: usize,
    ) -> Result<(Vec<Match>, Vec<Match>)> {
        let mut example_matches = Vec::new();
        let mut other_matches = Vec::new();

        self.search_directory(crate_path, crate_path, pattern, context_lines, &mut example_matches, &mut other_matches)?;

        Ok((example_matches, other_matches))
    }

    /// Recursively search a directory
    fn search_directory(
        &self,
        base_path: &Path,
        current_path: &Path,
        pattern: &Regex,
        context_lines: usize,
        example_matches: &mut Vec<Match>,
        other_matches: &mut Vec<Match>,
    ) -> Result<()> {
        for entry in fs::read_dir(current_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Skip hidden directories and target directories
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || name == "target" {
                        continue;
                    }
                }
                self.search_directory(base_path, &path, pattern, context_lines, example_matches, other_matches)?;
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                // Search Rust files
                if let Ok(matches) = self.search_file(base_path, &path, pattern, context_lines) {
                    let is_example = self.is_example_file(base_path, &path);
                    if is_example {
                        example_matches.extend(matches);
                    } else {
                        other_matches.extend(matches);
                    }
                }
            }
        }

        Ok(())
    }

    /// Search a single file for the pattern
    fn search_file(
        &self,
        base_path: &Path,
        file_path: &Path,
        pattern: &Regex,
        context_lines: usize,
    ) -> Result<Vec<Match>> {
        let content = fs::read_to_string(file_path)
            .map_err(|e| EgError::Other(format!("Failed to read file {}: {}", file_path.display(), e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let mut matches = Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            if pattern.is_match(line) {
                let line_number = (line_idx + 1) as u32; // 1-based line numbers
                
                // Get context lines
                let context_start = line_idx.saturating_sub(context_lines);
                let context_end = std::cmp::min(line_idx + context_lines + 1, lines.len());
                
                let context_before = lines[context_start..line_idx]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                
                let context_after = lines[line_idx + 1..context_end]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // Get relative path from base
                let relative_path = file_path.strip_prefix(base_path)
                    .unwrap_or(file_path)
                    .to_path_buf();

                matches.push(Match {
                    file_path: relative_path,
                    line_number,
                    line_content: line.to_string(),
                    context_before,
                    context_after,
                });
            }
        }

        Ok(matches)
    }

    /// Check if a file is in the examples directory
    fn is_example_file(&self, base_path: &Path, file_path: &Path) -> bool {
        if let Ok(relative_path) = file_path.strip_prefix(base_path) {
            relative_path.components().any(|c| c.as_os_str() == "examples")
        } else {
            false
        }
    }
}
