//! Running a PM written against the SDK in this process.
//!
//! [`RemotePm`](super::RemotePm) is one way to reach a
//! [`PackageManagerServer`]: spawn it and speak the protocol. [`LocalPm`] is
//! the other: call it directly. Same trait on the PM's side, so a PM author
//! writes one implementation and Symposium decides where it runs.
//!
//! That decision is not cosmetic. The cargo PM is the one Symposium cannot do
//! without, so it runs in-process by default: no second binary to find, no
//! spawn on the hook path. A PM configured by the user runs out of process,
//! because nothing else can contain what a third party's code does.

use anyhow::Result;
use symposium_install::UpdateLevel;
use symposium_sdk::pm::server::PackageManagerServer;

use super::update_of;
use super::{FetchedPackage, PackageId, PackageManager, PluginInfo, PluginOffer, WorkspaceInfo};

/// Adapts a [`PackageManagerServer`] to Symposium's [`PackageManager`].
///
/// The two traits are the same operation set in different vocabularies: the
/// SDK's is what a PM author implements, and it deliberately cannot name
/// Symposium's internal types.
pub struct LocalPm<P>(pub P);

impl<P: PackageManagerServer> LocalPm<P> {
    pub fn new(pm: P) -> Self {
        Self(pm)
    }
}

#[async_trait::async_trait]
impl<P: PackageManagerServer + Send + Sync> PackageManager for LocalPm<P> {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn active_plugins(&self, deps: &[PackageId]) -> Vec<PluginOffer> {
        self.0.active_plugins(deps).await
    }

    async fn load_plugin(&self, id: &PackageId) -> Vec<PluginOffer> {
        self.0.load_plugin(id).await
    }

    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        self.0.list_deps().await
    }

    async fn workspace_info(&self) -> Result<Option<WorkspaceInfo>> {
        self.0.workspace_info().await
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        self.0.search(query).await
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let fetched = self.0.fetch(id, update_of(update)).await?;
        Ok(FetchedPackage {
            id: fetched.id,
            root: fetched.root,
        })
    }

    async fn refresh(&self, update: UpdateLevel, force: bool) -> Result<bool> {
        self.0.refresh(update_of(update), force).await
    }
}
