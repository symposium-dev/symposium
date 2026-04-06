//! A tiny cross-platform test helper binary.
//!
//! Usage: write_file <path> <content>
//!
//! Used in tests as a cross-platform alternative to `sh -c 'echo ... > file'`

fn main() {
    let args: Vec<String> = std::env::args().collect();

    assert!(args.len() == 3, "Usage: test_helper <path> <content>");

    std::fs::write(&args[1], &args[2])
        .unwrap_or_else(|err| panic!("failed to write to {}: {}", &args[1], err));
}
