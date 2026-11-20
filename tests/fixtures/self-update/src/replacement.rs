//! The newly installed application: observe restart arguments, stdin and status.

use std::io::{self, Read, Write};

pub fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    println!("replacement: {}", args.join("|"));
    io::stdout().flush().unwrap();
    if args
        .first()
        .is_some_and(|arg| arg == "hook" || arg == "hold")
    {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input).unwrap();
        println!("stdin: {input}");
    }
    std::process::exit(42);
}
