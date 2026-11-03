//! Reading JSON Lines that another program wrote, one bounded line at a time.
//!
//! Transcripts and hook traces are written inside a trial, so a line's size is the
//! writer's to choose. A line longer than [`MAX_LINE`] is skipped to its newline
//! without being held, and reading goes on past it.

use std::io::{self, BufRead, Read};

/// The longest line read, without its newline. Recorded lines reach tens of KiB, from
/// large tool results; this leaves room for far larger ones.
pub(crate) const MAX_LINE: usize = 8 * 1024 * 1024;

/// What reading one line found.
pub(crate) enum Line {
    End,
    Read,
    /// Longer than [`MAX_LINE`]; the line was skipped.
    TooLong,
}

/// Reads the next line of `source` into `line`, holding at most [`MAX_LINE`] bytes of
/// it.
pub(crate) fn next_line(source: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    line.clear();
    let limit = MAX_LINE as u64 + 1;
    if source.by_ref().take(limit).read_until(b'\n', line)? == 0 {
        return Ok(Line::End);
    }
    if line.len() > MAX_LINE && !line.ends_with(b"\n") {
        // Drop the rest of the line rather than hold it.
        source.skip_until(b'\n')?;
        return Ok(Line::TooLong);
    }
    Ok(Line::Read)
}
