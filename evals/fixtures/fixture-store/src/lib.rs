//! A small key/value store kept in one file in a directory.
//!
//! Exists only in this repository, so an agent cannot already know how it behaves.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "store.txt";

/// A key/value store backed by `store.txt` in its directory.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    entries: BTreeMap<String, String>,
}

impl Store {
    /// Opens the store in `dir`, reading what an earlier `Store` flushed there.
    ///
    /// # Errors
    ///
    /// When the store file exists but cannot be read.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(FILE_NAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let entries = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        Ok(Self { path, entries })
    }

    /// Sets `key` to `value` in memory.
    ///
    /// # Errors
    ///
    /// When `key` or `value` holds a tab or a line break, which the file cannot store.
    pub fn put(&mut self, key: &str, value: &str) -> io::Result<()> {
        if [key, value]
            .iter()
            .any(|text| text.contains(['\t', '\n', '\r']))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "keys and values cannot hold tabs or line breaks",
            ));
        }
        self.entries.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    /// The value of `key`, including writes not yet flushed.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Writes every entry to the store file.
    ///
    /// # Errors
    ///
    /// When the store file cannot be written.
    pub fn flush(&mut self) -> io::Result<()> {
        let text: String = self
            .entries
            .iter()
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect();
        fs::write(&self.path, text)
    }
}
