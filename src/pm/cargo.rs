//! The cargo package manager: crates from the active workspace's dependency
//! graph, resolved by [`RustCrateFetch`] (path-dependency override, then the
//! cargo registry cache, then crates.io).

use anyhow::Result;
use symposium_sdk::workspace::WorkspaceCrate;

use crate::crate_sources::RustCrateFetch;

use super::{ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager};

pub struct CargoPm;

impl CargoPm {
    /// Cargo id for a crate name and optional version requirement.
    pub fn id_for(name: &str, version: Option<&str>) -> PackageId {
        PackageId::new(CARGO_PM, name, version.unwrap_or(ANY_VERSION))
    }
}

impl PackageManager for CargoPm {
    async fn fetch(&self, id: &PackageId, workspace: &[WorkspaceCrate]) -> Result<FetchedPackage> {
        debug_assert_eq!(id.pm, CARGO_PM);
        let mut fetch = RustCrateFetch::new(&id.name, workspace);
        if id.version != ANY_VERSION {
            fetch = fetch.version(&id.version);
        }
        let result = fetch.fetch().await?;
        Ok(FetchedPackage {
            id: PackageId::new(CARGO_PM, result.name, result.version),
            root: result.path,
        })
    }
}
