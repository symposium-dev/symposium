//! Convert one-pass predicate evidence into safe resolution witnesses.
//!
//! The predicate layer reports the exact successful branch with its sourced
//! package still attached. This module is the privacy boundary that applies
//! public naming policy and replaces everything else with fixed opaque nodes.
//! It never re-evaluates a predicate.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the structured sync report consumes resolution witnesses in the next commit"
    )
)]

use crate::{
    pm::SourcedPackage,
    predicate::{
        PredicateEvaluationMode, PredicateWitnessEval, SourcedDependencies, SourcedPredicateContext,
    },
};
use symposium_sdk::predicate::syntax::PredicateSet;

use super::{
    policy::{ExtensionNaming, PackageNaming},
    schema::ResolutionWitness,
};

/// Evaluate a predicate set once and retain only safe evidence from that pass.
///
/// A successful empty set returns an empty witness so its parent can flatten
/// the set without inventing a wire node.
pub(in crate::telemetry) fn evaluate_predicates<'a>(
    predicates: &PredicateSet,
    context: &mut SourcedPredicateContext<'a>,
) -> Option<ResolutionWitness> {
    predicates.evaluate_witness(context, &mut SafeResolutionMode)
}

/// Convert one package naming decision into a safe path fragment.
#[must_use]
pub(in crate::telemetry) fn package(naming: &PackageNaming) -> ResolutionWitness {
    match naming {
        PackageNaming::Public(coordinate) => ResolutionWitness::package(coordinate.clone()),
        PackageNaming::Unnamed(_) => ResolutionWitness::private_source(),
    }
}

/// Convert one extension naming decision into a safe path fragment.
#[must_use]
pub(in crate::telemetry) fn extension(naming: &ExtensionNaming) -> ResolutionWitness {
    match naming {
        ExtensionNaming::Public(coordinate) => ResolutionWitness::extension(coordinate.clone()),
        ExtensionNaming::Unnamed => ResolutionWitness::private_source(),
    }
}

struct SafeResolutionMode;

impl<'a> PredicateEvaluationMode<SourcedDependencies<'a>> for SafeResolutionMode {
    type Evidence = ResolutionWitness;

    fn dependency(&mut self, dependency: &SourcedPackage) -> Self::Evidence {
        package(&super::policy::package(dependency))
    }

    fn non_package(&mut self) -> Self::Evidence {
        ResolutionWitness::non_package_predicate()
    }

    fn not(&mut self) -> Self::Evidence {
        ResolutionWitness::not()
    }

    fn any(&mut self, child: Self::Evidence) -> Self::Evidence {
        ResolutionWitness::any(child)
    }

    fn all(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        ResolutionWitness::all(children)
    }

    fn set(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        let mut witness = ResolutionWitness::empty();
        for child in children {
            witness.append(child);
        }
        witness
    }
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

    #[test]
    fn public_dependency_evidence_keeps_only_the_reviewed_coordinate() {
        let packages = vec![crates_io("example-runtime")];
        let mut context = SourcedPredicateContext::new(&packages);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn("example-runtime".to_owned(), None)],
        };

        let witness = evaluate_predicates(&predicates, &mut context).unwrap();

        assert_eq!(
            path_json(witness),
            json!([{
                "type": "package",
                "ecosystem": "cargo",
                "name": "example-runtime",
                "version": "1.2.3"
            }])
        );
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
            &evaluate_predicates(&predicates, &mut context)
                .unwrap()
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

        let witness = evaluate_predicates(&predicates, &mut context).unwrap();

        assert_eq!(
            path_json(witness),
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

        let witness = evaluate_predicates(&predicates, &mut context).unwrap();

        assert_eq!(witness.into_path(), None);
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
            path_json(extension(&public)),
            json!([{
                "type": "extension",
                "extension_type": "skill",
                "source": "crates-io",
                "name": "example-debugging"
            }])
        );
        assert_eq!(
            path_json(extension(&unnamed)),
            json!([{ "type": "opaque", "reason": "private_source" }])
        );
    }

    #[test]
    fn composition_preserves_package_predicate_and_extension_order() {
        let package_naming = policy::package(&crates_io("example-runtime"));
        let extension_naming = policy::extension(
            &PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
            ExtensionKind::Plugin,
            "example-tools",
        );
        let mut witness = package(&package_naming);
        witness.append(ResolutionWitness::not());
        witness.append(extension(&extension_naming));

        let encoded = serde_json::to_string(&witness.into_path().unwrap()).unwrap();

        assert_eq!(
            encoded,
            r#"[{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},{"type":"not"},{"type":"extension","extension_type":"plugin","source":"crates-io","name":"example-tools"}]"#
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
        let depth_json = path_json(evaluate_predicates(&depth_set, &mut empty_context).unwrap());
        let leaf_json = path_json(evaluate_predicates(&leaf_set, &mut empty_context).unwrap());
        let size_json = path_json(package(&policy::package(&oversized)));

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
                path_json(package(&PackageNaming::Unnamed(reason))),
                json!([{ "type": "opaque", "reason": "private_source" }])
            );
        }
    }
}
