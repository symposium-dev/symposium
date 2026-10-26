//! Paths grading always takes from the starter, whatever `editable` says.
//!
//! Each one changes how Cargo builds the crate, runs code while it builds, or how
//! rustfmt judges it, so a submission that could replace one could change what grading
//! measures. rustfmt reads a config in every directory above a source file, so a
//! `rustfmt.toml` beside the code would loosen the fmt check. Grading never
//! imports them; `--check-tasks` rejects an `editable` pattern that would match one, so
//! an author learns early instead of from a silently ignored file.
//!
//! Names compare without regard to ASCII case on every platform: on the case-insensitive
//! filesystems macOS and Windows use by default, `cargo.toml` is `Cargo.toml`.
//!
//! A build script at a custom path (`package.build` in a manifest) is not visible from
//! the path alone; grading adds those from `cargo metadata` before running any command
//! that could execute one.

/// Components that protect a path wherever they appear. Every `build.rs` counts, not only
/// the root's: each package's default build script sits at its own root, and a workspace
/// member's root is a subdirectory.
const PROTECTED_COMPONENTS: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    ".cargo",
    "rust-toolchain",
    "rust-toolchain.toml",
    "build.rs",
    "rustfmt.toml",
    ".rustfmt.toml",
];

/// Whether `path`, relative to the project root with `/` separators, is protected.
pub(crate) fn is_protected(path: &str) -> bool {
    path.split('/').any(|component| {
        PROTECTED_COMPONENTS
            .iter()
            .any(|protected| component.eq_ignore_ascii_case(protected))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_files_are_protected_at_any_depth() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "member/Cargo.toml",
            ".cargo",
            ".cargo/config.toml",
            "nested/.cargo/config.toml",
            "rust-toolchain",
            "nested/rust-toolchain.toml",
            "build.rs",
            "member/build.rs",
            "src/rustfmt.toml",
            "src/nested/.rustfmt.toml",
        ] {
            assert!(is_protected(path), "{path}");
        }
    }

    #[test]
    fn protection_ignores_ascii_case() {
        for path in [
            "cargo.toml",
            "src/CARGO.LOCK",
            ".Cargo/config.toml",
            "BUILD.RS",
            "Rust-Toolchain.TOML",
        ] {
            assert!(is_protected(path), "{path}");
        }
    }

    #[test]
    fn ordinary_sources_are_not_protected() {
        for path in [
            "src/lib.rs",
            "src/cargo.rs",
            "src/build_script.rs",
            "README.md",
        ] {
            assert!(!is_protected(path), "{path}");
        }
    }
}
