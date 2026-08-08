//! Building a crate's plugin manifest from what the crate ships.

use symposium_sdk::manifest::RawPluginManifest;

/// Merge a crate's two manifest sources into one, for the cargo PM to offer.
///
/// A crate can describe its plugin two ways, and this combines them (later
/// layers win / append):
/// 1. `[package.metadata.symposium]` from `Cargo.toml` (`metadata`);
/// 2. a `SYMPOSIUM.toml` file at the crate root (`file`).
///
/// Both use the same schema as any plugin manifest. Each is parsed
/// independently and **leniently**: a malformed layer is logged and dropped so
/// the crate still resolves through the remaining layers. A crate with neither
/// yields an empty manifest, which validation still turns into a plugin
/// carrying the default `skills/` group, so any fetchable crate is offerable.
///
/// The default group itself is appended by validation on Symposium's side, not
/// here: defaults are policy, and this crate holds none.
pub fn merge(
    metadata: Option<toml::Table>,
    file: Option<&str>,
    crate_name: &str,
) -> RawPluginManifest {
    let meta = metadata.and_then(
        |t| match toml::Value::Table(t).try_into::<RawPluginManifest>() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(
                    crate_name = %crate_name,
                    error = %e,
                    "ignoring malformed [package.metadata.symposium]"
                );
                None
            }
        },
    );
    let file = file.and_then(|c| match toml::from_str::<RawPluginManifest>(c) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(
                crate_name = %crate_name,
                error = %e,
                "ignoring malformed crate SYMPOSIUM.toml"
            );
            None
        }
    });
    match (meta, file) {
        (Some(a), Some(b)) => a.merge(b),
        (Some(m), None) | (None, Some(m)) => m,
        (None, None) => RawPluginManifest::default(),
    }
}
