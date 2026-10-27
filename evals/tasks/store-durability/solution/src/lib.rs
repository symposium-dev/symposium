//! Application settings, saved with fixture-store.

use std::io;
use std::path::Path;

use fixture_store::Store;

/// Saves each key/value pair in the store at `dir`.
///
/// # Errors
///
/// When the store cannot be opened or written.
pub fn save_settings(dir: &Path, settings: &[(&str, &str)]) -> io::Result<()> {
    let mut store = Store::open(dir)?;
    for (key, value) in settings {
        store.put(key, value)?;
    }
    store.flush()
}
