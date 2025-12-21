//! Error types for the Ferris library

use thiserror::Error;

/// Result type alias for Ferris operations
pub type Result<T> = std::result::Result<T, FerrisError>;

/// Errors that can occur during Ferris operations
#[derive(Debug, Error)]
pub enum FerrisError {
    /// Failed to parse or access project metadata
    #[error("Project error: {0}")]
    ProjectError(#[from] cargo_metadata::Error),
    /// Failed to resolve version constraints
    #[error("Version error: {0}")]
    VersionError(#[from] semver::Error),
    /// Could not determine CARGO_HOME directory
    #[error("Could not determine CARGO_HOME directory")]
    CargoHomeNotFound(#[source] std::io::Error),
    /// Failed to download crate from registry
    #[error("Download error: {0}")]
    DownloadError(#[from] reqwest::Error),
    /// Failed to extract or process crate archive
    #[error("Extraction error: {0}")]
    ExtractionError(String),
    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    /// Crate not found
    #[error("Crate '{0}' not found")]
    CrateNotFound(String),
    /// No matching versions found
    #[error("No versions of '{crate_name}' match constraint '{constraint}'")]
    NoMatchingVersions {
        crate_name: String,
        constraint: String,
    },
    /// Other error
    #[error("Error: {0}")]
    Other(String),
}
