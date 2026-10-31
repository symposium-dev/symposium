//! Convert one-pass predicate evidence into safe resolution witnesses.
//!
//! The predicate layer reports the exact successful branch with its sourced
//! package still attached. This module is the privacy boundary that applies
//! public naming policy and replaces everything else with fixed opaque nodes.
//! It never re-evaluates a predicate.
use std::collections::BTreeSet;

use crate::{
    pm::{PackageProvenance, SourcedPackage},
    predicate::{
        PredicateEvaluationMode, PredicateWitnessEval, SourcedDependencies, SourcedPredicateContext,
    },
};
use symposium_sdk::predicate::syntax::PredicateSet;

use super::{
    policy::{ExtensionNaming, PackageNaming},
    schema::{PublicExtensionCoordinate, ResolutionPath, ResolutionWitness},
};

/// Private one-pass resolution evidence.
///
/// This working value deliberately has neither `Debug` nor serialization: its
/// contributor set retains raw package provenance, including private URLs.
/// Only the witness has passed the public naming boundary.
#[derive(Clone)]
pub(crate) struct ResolutionEvidence {
    contributors: BTreeSet<SourcedPackage>,
    witness: ResolutionWitness,
    target: Option<PublicExtensionCoordinate>,
}

impl ResolutionEvidence {
    fn empty() -> Self {
        Self {
            contributors: BTreeSet::new(),
            witness: ResolutionWitness::empty(),
            target: None,
        }
    }

    fn from_witness(witness: ResolutionWitness) -> Self {
        Self {
            contributors: BTreeSet::new(),
            witness,
            target: None,
        }
    }

    fn package(package: &SourcedPackage) -> Self {
        Self {
            contributors: BTreeSet::from([package.clone()]),
            witness: package_witness(&super::policy::package(package)),
            target: None,
        }
    }

    fn append(&mut self, other: Self) {
        self.contributors.extend(other.contributors);
        self.witness.append(other.witness);
    }

    fn append_extension(&mut self, naming: &ExtensionNaming) {
        self.target = match naming {
            ExtensionNaming::Public(coordinate) => Some(coordinate.clone()),
            ExtensionNaming::Unnamed => None,
        };
        self.witness.append(extension_witness(naming));
    }

    pub(crate) fn union_contributors(&mut self, other: &Self) {
        self.contributors.extend(other.contributors.iter().cloned());
    }

    #[cfg(test)]
    pub(crate) fn contributors(&self) -> &BTreeSet<SourcedPackage> {
        &self.contributors
    }

    pub(super) fn contributes(&self, package: &SourcedPackage) -> bool {
        self.contributors.contains(package)
    }

    pub(super) fn names_public_extension(&self) -> bool {
        self.target.is_some()
    }

    pub(super) fn safe_resolution(&self) -> Option<SafeExtensionResolution> {
        Some(SafeExtensionResolution {
            target: self.target.clone()?,
            path: self.witness.clone().into_path()?,
        })
    }

    #[cfg(test)]
    fn into_witness(self) -> ResolutionWitness {
        self.witness
    }
}

/// Public extension target and bounded path safe to retain in a sync report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeExtensionResolution {
    pub(super) target: PublicExtensionCoordinate,
    pub(super) path: ResolutionPath,
}

/// Runtime-selectable evidence mode for the sync traversal.
///
/// With retention disabled, every successful node produces `None`: predicate
/// truth and short-circuiting are unchanged, but no naming policy runs and no
/// evidence vector is allocated.
pub(crate) struct OptionalWitnessMode {
    retain: bool,
}

impl OptionalWitnessMode {
    #[must_use]
    pub(crate) const fn new(retain: bool) -> Self {
        Self { retain }
    }

    #[must_use]
    pub(crate) fn start(&self) -> Option<ResolutionEvidence> {
        self.retain.then(ResolutionEvidence::empty)
    }
}

/// Evaluate a predicate set once and optionally retain safe evidence.
///
/// The outer option is predicate success. The inner option is present only
/// when evidence retention is enabled. A successful empty set retains an
/// empty witness so its parent can flatten the set without inventing a node.
pub(crate) fn evaluate_predicates<'a>(
    predicates: &PredicateSet,
    context: &mut SourcedPredicateContext<'a>,
    mode: &mut OptionalWitnessMode,
) -> Option<Option<ResolutionEvidence>> {
    predicates.evaluate_witness(context, mode)
}

/// Convert one package naming decision into a safe path fragment.
#[must_use]
fn package_witness(naming: &PackageNaming) -> ResolutionWitness {
    match naming {
        PackageNaming::Public(coordinate) => ResolutionWitness::package(coordinate.clone()),
        PackageNaming::Unnamed(_) => ResolutionWitness::private_source(),
    }
}

/// Convert one extension naming decision into a safe path fragment.
#[must_use]
fn extension_witness(naming: &ExtensionNaming) -> ResolutionWitness {
    match naming {
        ExtensionNaming::Public(coordinate) => ResolutionWitness::extension(coordinate.clone()),
        ExtensionNaming::Unnamed => ResolutionWitness::private_source(),
    }
}

impl<'a> PredicateEvaluationMode<SourcedDependencies<'a>> for OptionalWitnessMode {
    type Evidence = Option<ResolutionEvidence>;

    fn retains_evidence(&self) -> bool {
        self.retain
    }

    fn dependency(&mut self, dependency: &SourcedPackage) -> Self::Evidence {
        self.retain.then(|| ResolutionEvidence::package(dependency))
    }

    fn non_package(&mut self) -> Self::Evidence {
        self.retain
            .then(|| ResolutionEvidence::from_witness(ResolutionWitness::non_package_predicate()))
    }

    fn not(&mut self) -> Self::Evidence {
        self.retain
            .then(|| ResolutionEvidence::from_witness(ResolutionWitness::not()))
    }

    fn any(&mut self, child: Self::Evidence) -> Self::Evidence {
        child.map(|mut child| {
            child.witness = ResolutionWitness::any(child.witness);
            child
        })
    }

    fn all(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        // An empty vector has two meanings here. Disabled traversal discards
        // every child result, while retained traversal can receive a literal
        // zero-child `all`. `self.retain` distinguishes those cases.
        if children.is_empty() {
            return self
                .retain
                .then(|| ResolutionEvidence::from_witness(ResolutionWitness::all(Vec::new())));
        }
        combine_children(children, ResolutionWitness::all)
    }

    fn set(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        let mut evidence = self.start()?;
        // `evaluate_all` pushes children only when this mode retains evidence,
        // so reaching this loop proves every successful child is `Some`.
        for child in children {
            evidence.append(child.expect("retained predicate child must carry evidence"));
        }
        Some(evidence)
    }
}

fn combine_children(
    children: Vec<Option<ResolutionEvidence>>,
    combine: impl FnOnce(Vec<ResolutionWitness>) -> ResolutionWitness,
) -> Option<ResolutionEvidence> {
    if children.is_empty() {
        return None;
    }

    let mut contributors = BTreeSet::new();
    let mut witnesses = Vec::with_capacity(children.len());
    for child in children {
        // A non-empty vector can only come from retained `evaluate_all`;
        // disabled traversal discards all child results before this boundary.
        let child = child.expect("retained predicate child must carry evidence");
        contributors.extend(child.contributors);
        witnesses.push(child.witness);
    }
    Some(ResolutionEvidence {
        contributors,
        witness: combine(witnesses),
        target: None,
    })
}

/// Complete one plugin route in contract order.
///
/// Package-backed plugins append package, predicate, then plugin nodes. Trust
/// roots omit the placeholder package and begin with their predicate witness.
pub(crate) fn plugin(
    route: Option<ResolutionEvidence>,
    package: Option<&SourcedPackage>,
    predicates: Option<ResolutionEvidence>,
    plugin_package: &SourcedPackage,
    plugin_name: &str,
) -> Option<ResolutionEvidence> {
    let mut route = route?;
    if let Some(package) = package {
        route.append(ResolutionEvidence::package(package));
    }
    if let Some(predicates) = predicates {
        route.append(predicates);
    }
    route.append_extension(&super::policy::extension(
        &plugin_package.provenance,
        super::schema::ExtensionKind::Plugin,
        plugin_name,
    ));
    Some(route)
}

/// Extend an already-selected plugin route with one successful chained edge.
pub(crate) fn chained(
    route: Option<ResolutionEvidence>,
    predicates: Option<ResolutionEvidence>,
) -> Option<ResolutionEvidence> {
    let mut route = route?;
    if let Some(predicates) = predicates {
        route.append(predicates);
    }
    Some(route)
}

/// Complete one skill route in plugin, group, skill, extension order.
pub(crate) fn skill(
    route: Option<ResolutionEvidence>,
    group_predicates: Option<ResolutionEvidence>,
    skill_predicates: Option<ResolutionEvidence>,
    provenance: &PackageProvenance,
    skill_name: &str,
) -> Option<ResolutionEvidence> {
    let mut route = route?;
    if let Some(predicates) = group_predicates {
        route.append(predicates);
    }
    if let Some(predicates) = skill_predicates {
        route.append(predicates);
    }
    route.append_extension(&super::policy::extension(
        provenance,
        super::schema::ExtensionKind::Skill,
        skill_name,
    ));
    Some(route)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use symposium_sdk::{pm::PackageId, predicate::syntax::Predicate};

    use super::*;
    use crate::{
        pm::{CARGO_PM, CRATES_IO_REGISTRY_URL, PackageProvenance},
        telemetry::{
            policy,
            schema::{ExtensionKind, UnnamedPackageReason},
        },
    };

    fn sourced(name: &str, version: &str, provenance: PackageProvenance) -> SourcedPackage {
        SourcedPackage::new(PackageId::new(CARGO_PM, name, version), provenance)
    }

    fn crates_io(name: &str) -> SourcedPackage {
        sourced(
            name,
            "1.2.3",
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        )
    }

    fn path_json(witness: ResolutionWitness) -> serde_json::Value {
        serde_json::to_value(
            witness
                .into_path()
                .expect("test witness should contain path evidence"),
        )
        .unwrap()
    }

    fn retained(
        predicates: &PredicateSet,
        context: &mut SourcedPredicateContext<'_>,
    ) -> ResolutionEvidence {
        evaluate_predicates(predicates, context, &mut OptionalWitnessMode::new(true))
            .expect("test predicates should match")
            .expect("retained evaluation should produce evidence")
    }

    fn evidence_json(evidence: ResolutionEvidence) -> serde_json::Value {
        path_json(evidence.into_witness())
    }

    #[test]
    fn public_dependency_evidence_keeps_only_the_reviewed_coordinate() {
        let packages = vec![crates_io("example-runtime")];
        let mut context = SourcedPredicateContext::new(&packages);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn("example-runtime".to_owned(), None)],
        };

        let evidence = retained(&predicates, &mut context);

        assert_eq!(
            evidence_json(evidence),
            json!([{
                "type": "package",
                "ecosystem": "cargo",
                "name": "example-runtime",
                "version": "1.2.3"
            }])
        );
    }

    #[test]
    fn disabled_evaluation_keeps_truth_without_building_evidence() {
        let packages = vec![crates_io("example-runtime")];
        let mut context = SourcedPredicateContext::new(&packages);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn("example-runtime".to_owned(), None)],
        };

        let evaluated = evaluate_predicates(
            &predicates,
            &mut context,
            &mut OptionalWitnessMode::new(false),
        );

        assert!(matches!(evaluated, Some(None)));
    }

    #[test]
    fn private_dependency_evidence_is_opaque_and_content_free() {
        let private_url = "https://registry.example.invalid/private";
        let private_name = "internal-runtime";
        let packages = vec![sourced(
            private_name,
            "9.9.9",
            PackageProvenance::Registry {
                url: private_url.to_owned(),
            },
        )];
        let mut context = SourcedPredicateContext::new(&packages);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn(private_name.to_owned(), None)],
        };

        let encoded = serde_json::to_string(
            &retained(&predicates, &mut context)
                .into_witness()
                .into_path()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(encoded, r#"[{"type":"opaque","reason":"private_source"}]"#);
        assert!(!encoded.contains(private_name));
        assert!(!encoded.contains(private_url));
    }

    #[test]
    fn predicate_sets_flatten_while_explicit_all_retains_its_node() {
        let packages = vec![crates_io("example-runtime")];
        let mut context = SourcedPredicateContext::new(&packages);
        context.set_workspace_member(true);
        let predicates = PredicateSet {
            predicates: vec![
                Predicate::DependsOn("example-runtime".to_owned(), None),
                Predicate::Any(vec![
                    Predicate::DependsOn("missing".to_owned(), None),
                    Predicate::WorkspaceMember,
                ]),
                Predicate::All(vec![
                    Predicate::DependsOnWildcard,
                    Predicate::Not(Box::new(Predicate::DependsOn("missing".to_owned(), None))),
                ]),
            ],
        };

        let evidence = retained(&predicates, &mut context);

        assert_eq!(
            evidence_json(evidence),
            json!([
                {
                    "type": "package",
                    "ecosystem": "cargo",
                    "name": "example-runtime",
                    "version": "1.2.3"
                },
                {
                    "type": "any",
                    "child": {
                        "type": "opaque",
                        "reason": "non_package_predicate"
                    }
                },
                {
                    "type": "all",
                    "children": [
                        {
                            "type": "opaque",
                            "reason": "non_package_predicate"
                        },
                        { "type": "not" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn empty_predicate_set_contributes_no_path_node() {
        let mut context = SourcedPredicateContext::new(&[]);
        let predicates = PredicateSet::default();

        let evidence = retained(&predicates, &mut context);

        assert_eq!(evidence.into_witness().into_path(), None);
    }

    #[test]
    fn empty_all_cannot_emit_a_reader_invalid_node() {
        assert_eq!(
            path_json(ResolutionWitness::all(Vec::new())),
            json!([{
                "type": "opaque",
                "reason": "non_package_predicate"
            }])
        );
    }

    #[test]
    fn public_and_unnamed_extensions_use_the_same_safe_boundary() {
        let public = policy::extension(
            &PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
            ExtensionKind::Skill,
            "example-debugging",
        );
        let unnamed = policy::extension(
            &PackageProvenance::Path,
            ExtensionKind::Skill,
            "private-debugging",
        );

        assert_eq!(
            path_json(extension_witness(&public)),
            json!([{
                "type": "extension",
                "extension_type": "skill",
                "source": "crates-io",
                "name": "example-debugging"
            }])
        );
        assert_eq!(
            path_json(extension_witness(&unnamed)),
            json!([{ "type": "opaque", "reason": "private_source" }])
        );
    }

    #[test]
    fn composition_preserves_package_predicate_and_extension_order() {
        let package = crates_io("example-runtime");
        let predicate = ResolutionEvidence::from_witness(ResolutionWitness::not());
        let evidence = plugin(
            Some(ResolutionEvidence::empty()),
            Some(&package),
            Some(predicate),
            &package,
            "example-tools",
        )
        .unwrap();

        let encoded = serde_json::to_string(&evidence.into_witness().into_path().unwrap()).unwrap();

        assert_eq!(
            encoded,
            r#"[{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},{"type":"not"},{"type":"extension","extension_type":"plugin","source":"crates-io","name":"example-tools"}]"#
        );
    }

    #[test]
    fn private_git_skill_source_cannot_inherit_the_host_package_label() {
        let host = crates_io("public-host");
        let plugin_route = plugin(
            Some(ResolutionEvidence::empty()),
            None,
            None,
            &host,
            "public-plugin",
        );
        let evidence = skill(
            plugin_route,
            None,
            None,
            &PackageProvenance::Git {
                url: "https://example.invalid/private-skills".to_owned(),
            },
            "example-debugging",
        )
        .unwrap();

        assert!(evidence.safe_resolution().is_none());
        assert_eq!(
            evidence_json(evidence),
            json!([
                {
                    "type": "extension",
                    "extension_type": "plugin",
                    "source": "crates-io",
                    "name": "public-plugin"
                },
                { "type": "opaque", "reason": "private_source" }
            ])
        );
    }

    #[test]
    fn depth_leaf_and_encoded_size_limits_degrade_to_opaque_limit() {
        let mut nested = Predicate::DependsOnWildcard;
        for _ in 0..9 {
            nested = Predicate::Any(vec![nested]);
        }
        let depth_set = PredicateSet {
            predicates: vec![nested],
        };
        let leaf_set = PredicateSet {
            predicates: vec![Predicate::DependsOnWildcard; 17],
        };
        let oversized = sourced(
            "example-runtime",
            &format!("1.2.3+{}", "a".repeat(4_096)),
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );

        let mut empty_context = SourcedPredicateContext::new(&[]);
        let depth_json = evidence_json(retained(&depth_set, &mut empty_context));
        let leaf_json = evidence_json(retained(&leaf_set, &mut empty_context));
        let size_json = path_json(package_witness(&policy::package(&oversized)));

        assert!(depth_json.to_string().contains(r#""reason":"limit""#));
        assert_eq!(leaf_json, json!([{ "type": "opaque", "reason": "limit" }]));
        assert_eq!(size_json, json!([{ "type": "opaque", "reason": "limit" }]));
    }

    #[test]
    fn every_unnamed_package_reason_uses_the_same_opaque_node() {
        for reason in [
            UnnamedPackageReason::PrivateRegistry,
            UnnamedPackageReason::Git,
            UnnamedPackageReason::Path,
            UnnamedPackageReason::Workspace,
            UnnamedPackageReason::UnknownSource,
            UnnamedPackageReason::InvalidCoordinate,
        ] {
            assert_eq!(
                path_json(package_witness(&PackageNaming::Unnamed(reason))),
                json!([{ "type": "opaque", "reason": "private_source" }])
            );
        }
    }
}
