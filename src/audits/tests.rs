use super::*;

use symposium_sdk::pm::{FetchedPackage, PackageManager};

use crate::pm::{PluginKind, PmInstance};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn digest(s: &str) -> Digest {
    s.parse().unwrap()
}

fn v(s: &str) -> semver::Version {
    semver::Version::parse(s).unwrap()
}

/// Write `audits/cargo/<name>.toml` under the list's root `dir`.
fn write_audits(dir: &Path, name: &str, text: &str) {
    let audits = dir.join("audits/cargo");
    std::fs::create_dir_all(&audits).unwrap();
    std::fs::write(audits.join(format!("{name}.toml")), text).unwrap();
}

#[test]
fn file_path_is_per_package_manager_lowercase_and_confined() {
    assert_eq!(
        audit_file_path("cargo", "widget-lib").unwrap(),
        Path::new("cargo/widget-lib.toml")
    );
    assert_eq!(
        audit_file_path("cargo", "Widget_Lib").unwrap(),
        Path::new("cargo/widget_lib.toml")
    );
    for (pm, name) in [
        ("cargo", ""),
        ("cargo", "../x"),
        ("cargo", "a/b"),
        ("cargo", "a.b"),
        ("cargo", "a b"),
        ("", "x"),
        ("../cargo", "x"),
    ] {
        assert_eq!(audit_file_path(pm, name), None, "{pm} {name}");
    }
}

/// The three answers, and their precedence: a revocation beats an audit, by
/// version even when there is no digest, and by digest even when the same
/// digest is audited. Only the package's own file is read: hyphens are kept,
/// and another package manager's directory is another package.
#[test]
fn lookup_answers_revoked_audited_or_not_audited() {
    let tmp = tempfile::tempdir().unwrap();
    write_audits(
        tmp.path(),
        "widget-lib",
        &format!(
            "format = 1\n\
             [[audit]]\ndigest = \"{A}\"\nversions = [\"1.4.2\"]\n\
             [[audit]]\ndigest = \"{B}\"\n\
             [[revoked]]\nversions = \"<1.0.0\"\nreason = \"malicious-crate\"\n\
             [[revoked]]\ndigest = \"{B}\"\nreason = \"hidden-behavior\"\n"
        ),
    );
    let index = AuditIndex::new(tmp.path());
    let lookup = |version: &str, d: Option<&str>| {
        index.lookup("cargo", "widget-lib", version, d.map(digest).as_ref())
    };
    let revoked = |reason| Answer::Revoked(Revocation { reason });

    assert_eq!(lookup("1.4.2", Some(A)), Answer::Audited);
    // `versions` on an audit is bookkeeping: another release with the same
    // payload is audited too.
    assert_eq!(lookup("9.0.0", Some(A)), Answer::Audited);
    assert_eq!(
        lookup("1.4.2", Some(B)),
        revoked(RevokeReason::HiddenBehavior)
    );
    assert_eq!(lookup("0.9.0", None), revoked(RevokeReason::MaliciousCrate));
    assert_eq!(
        lookup("0.9.0", Some(A)),
        revoked(RevokeReason::MaliciousCrate)
    );
    // No digest, or one nobody listed, or no file at all.
    assert_eq!(lookup("1.4.2", None), Answer::NotAudited);
    assert_eq!(
        lookup("1.4.2", Some(&format!("sha256:{}", "c".repeat(64)))),
        Answer::NotAudited
    );
    for (pm, name) in [
        ("cargo", "gadget"),
        ("cargo", "widget_lib"),
        ("npm", "widget-lib"),
    ] {
        assert_eq!(
            index.lookup(pm, name, "0.9.0", Some(&digest(A))),
            Answer::NotAudited,
            "{pm} {name}"
        );
    }
    // No list at all.
    assert_eq!(
        AuditIndex::default().lookup("cargo", "widget-lib", "0.9.0", None),
        Answer::NotAudited
    );
}

/// A requirement matches as semver says, and a version that is not semver
/// never matches one. A pre-release is also matched as its release, since
/// semver's own rule would let every pre-release dodge a revocation.
#[test]
fn version_revocations_follow_semver_requirements() {
    let (audits, _) = parse_file(
        "format = 1\n[[revoked]]\nversions = \">=1.0.0, <2.0.0\"\nreason = \"off-task\"\n",
    );
    let audits = audits.unwrap();
    assert!(audits.revocation(Some(&v("1.5.0")), None).is_some());
    assert!(audits.revocation(Some(&v("1.5.0-rc.1")), None).is_some());
    assert!(audits.revocation(Some(&v("2.0.0")), None).is_none());
    assert!(audits.revocation(Some(&v("2.0.0-rc.1")), None).is_none());

    let tmp = tempfile::tempdir().unwrap();
    write_audits(
        tmp.path(),
        "w",
        "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n",
    );
    assert_eq!(
        AuditIndex::new(tmp.path()).lookup("cargo", "w", "latest", None),
        Answer::NotAudited
    );
}

/// `*` revokes every release, pre-releases included, and `>=1.0.0` covers a
/// later release's pre-release.
#[test]
fn a_range_revocation_covers_pre_releases() {
    let (audits, _) =
        parse_file("format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"malicious-crate\"\n");
    let audits = audits.unwrap();
    assert!(audits.revocation(Some(&v("2.0.0-alpha.1")), None).is_some());
    assert!(audits.revocation(Some(&v("0.0.1-0")), None).is_some());

    let (audits, _) =
        parse_file("format = 1\n[[revoked]]\nversions = \">=1.0.0\"\nreason = \"off-task\"\n");
    let audits = audits.unwrap();
    assert!(audits.revocation(Some(&v("1.5.0-rc.1")), None).is_some());
    assert!(audits.revocation(Some(&v("0.9.0-rc.1")), None).is_none());
}

/// What a file says, what kept `(audits, revocations)` it yields (`None`
/// ignores it whole), and a fragment of each warning, in order.
type Case<'a> = (&'a str, String, Option<(usize, usize)>, Vec<&'a str>);

/// Whatever is malformed is dropped with a warning that says what and why,
/// and only that: the rest of the file still counts. A file whose `format`
/// is missing or unknown, or that is not TOML, is ignored whole.
#[test]
fn malformed_input_is_dropped_leniently() {
    let revoke_all = "[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n";
    // (what, file text, audits kept, revocations kept, warning fragments)
    let cases: Vec<Case> = vec![
        (
            "unknown format",
            format!("format = 2\n{revoke_all}"),
            None,
            vec!["unsupported format 2"],
        ),
        (
            "missing format",
            revoke_all.to_string(),
            None,
            vec!["unsupported format (missing)"],
        ),
        (
            "a format that is not an integer",
            "format = \"1\"\n".to_string(),
            None,
            vec!["unsupported format"],
        ),
        (
            "invalid TOML",
            "format = [".to_string(),
            None,
            vec!["not valid TOML"],
        ),
        (
            "a `revoked` key that is not an array of tables",
            format!(
                "format = 1\n[revoked]\nversions = \"<2.0.0\"\nreason = \"off-task\"\n\
                 [[audit]]\ndigest = \"{A}\"\n"
            ),
            Some((1, 0)),
            vec!["ignoring `revoked`: expected an array of tables ([[revoked]])"],
        ),
        (
            "malformed entries among good ones, and unknown keys",
            format!(
                "format = 1\ngenerator = \"ignored\"\n\
                 [[audit]]\ndigest = \"not-a-digest\"\n\
                 [[audit]]\ndigest = 5\n\
                 [[audit]]\ndigest = \"{A}\"\nreviewer = \"ignored\"\n\
                 [[revoked]]\nversions = \"not a requirement\"\nreason = \"off-task\"\n\
                 [[revoked]]\nversions = \"<1.0.0\"\ndigest = \"{B}\"\nreason = \"off-task\"\n\
                 [[revoked]]\nreason = \"off-task\"\n\
                 [[revoked]]\ndigest = \"sha256:short\"\nreason = \"off-task\"\n\
                 [[revoked]]\nversions = \"<3.0.0\"\n\
                 [[revoked]]\nversions = \"<3.0.0\"\nreason = \"report\"\n\
                 [[revoked]]\nversions = \"<2.0.0\"\nreason = \"malicious-crate\"\n"
            ),
            // An unknown reason code is not malformed: it still revokes.
            Some((1, 2)),
            vec![
                "dropping [[audit]] entry #1",
                "dropping [[audit]] entry #2: `digest` must be a string",
                "dropping [[revoked]] entry #1: `versions = \"not a requirement\"`",
                "dropping [[revoked]] entry #2: names both",
                "dropping [[revoked]] entry #3: names neither",
                "dropping [[revoked]] entry #4",
                "dropping [[revoked]] entry #5",
                "`report`, which this symposium does not know; it still revokes",
            ],
        ),
        (
            "a malformed audit `versions`, which is never read",
            format!("format = 1\n[[audit]]\ndigest = \"{A}\"\nversions = [1, {{ x = 1 }}]\n"),
            Some((1, 0)),
            vec![],
        ),
    ];
    for (what, text, kept, expected) in cases {
        let (audits, warnings) = parse_file(&text);
        assert_eq!(
            audits.map(|a| (a.audited.len(), a.revoked.len())),
            kept,
            "{what}"
        );
        assert_eq!(warnings.len(), expected.len(), "{what}: {warnings:?}");
        for (warning, fragment) in warnings.iter().zip(&expected) {
            assert!(
                warning.contains(fragment),
                "{what}: {warning:?} lacks {fragment:?}"
            );
        }
    }

    // A missing file, a directory in its place, or a file that is not UTF-8
    // is no audits (and no panic).
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("audits/cargo/a-dir.toml")).unwrap();
    let mut not_utf8 = b"format = 1\n# \xff\n".to_vec();
    not_utf8.extend_from_slice(revoke_all.as_bytes());
    std::fs::write(tmp.path().join("audits/cargo/not-utf8.toml"), not_utf8).unwrap();
    let index = AuditIndex::new(tmp.path());
    for name in ["missing", "a-dir", "not-utf8"] {
        assert!(index.for_package("cargo", name).is_empty(), "{name}");
    }
}

/// A file of at most [`MAX_FILE_BYTES`] is read; a larger one is ignored
/// whole, revocations included.
#[test]
fn the_file_size_cap_is_exact() {
    let entry = "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n";
    let padded = |len: u64| {
        let mut text = format!("{entry}#");
        text.push_str(&"x".repeat(len as usize - text.len() - 1));
        text.push('\n');
        assert_eq!(text.len() as u64, len);
        text
    };
    let tmp = tempfile::tempdir().unwrap();
    let index = AuditIndex::new(tmp.path());

    write_audits(tmp.path(), "at-cap", &padded(MAX_FILE_BYTES));
    assert_eq!(index.for_package("cargo", "at-cap").revoked.len(), 1);

    write_audits(tmp.path(), "over-cap", &padded(MAX_FILE_BYTES + 1));
    assert!(index.for_package("cargo", "over-cap").is_empty());
}

/// A cargo package manager whose only package is at `root`, whatever version
/// is asked for; with no `root`, it must never be asked for a payload.
struct OnePackage {
    root: Option<PathBuf>,
}

#[async_trait::async_trait]
impl PackageManager for OnePackage {
    fn name(&self) -> &str {
        CARGO_PM
    }

    async fn load_plugin(
        &self,
        _id: &PackageId,
        _update: UpdateLevel,
    ) -> anyhow::Result<Vec<symposium_sdk::pm::UnvalidatedPlugin>> {
        Ok(Vec::new())
    }

    async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> anyhow::Result<FetchedPackage> {
        let Some(root) = &self.root else {
            panic!("{id} was fetched to be digested, but no entry could match a digest")
        };
        anyhow::ensure!(root.exists(), "not cached");
        // Resolve an exact requirement to its release, as a real one does.
        let version = id.version.strip_prefix('=').unwrap_or(&id.version);
        Ok(FetchedPackage {
            id: PackageId::new(&id.pm, &id.name, version),
            root: root.clone(),
        })
    }
}

fn package_at(root: Option<&Path>) -> PmRegistry {
    PmRegistry::new(vec![PmInstance::new(
        CARGO_PM,
        false,
        PluginKind::Package,
        Box::new(OnePackage {
            root: root.map(Path::to_path_buf),
        }),
    )])
}

/// `AuditIndex::revocation` matches a loaded cargo package by version or by
/// the digest of the payload its package manager holds, and only a cargo
/// package.
#[tokio::test]
async fn revocation_matches_a_loaded_cargo_package_by_version_or_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let krate = tmp.path().join("crate");
    std::fs::create_dir_all(krate.join("skills/s")).unwrap();
    std::fs::write(krate.join("skills/s/SKILL.md"), "---\nname: s\n---\n").unwrap();
    let payload = digest::payload_digest(&krate).unwrap();

    let list = tmp.path().join("list");
    write_audits(
        &list,
        "widget-lib",
        &format!(
            "format = 1\n\
             [[revoked]]\nversions = \">=2.0.0, <2.1.0\"\nreason = \"malicious-crate\"\n\
             [[revoked]]\ndigest = \"{payload}\"\nreason = \"remote-exec\"\n"
        ),
    );
    let index = AuditIndex::new(&list);
    let id = |version: &str| PackageId::new(CARGO_PM, "widget-lib", version);

    // By payload, whatever the version (including one that is not semver).
    let pms = package_at(Some(&krate));
    let by_payload = index.revocation(&pms, &id("9.9.9")).await.unwrap();
    assert_eq!(by_payload.reason, RevokeReason::RemoteExec);
    assert!(index.revocation(&pms, &id("*")).await.is_some());

    // By version, when the payload differs.
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let pms = package_at(Some(&other));
    let hit = index.revocation(&pms, &id("2.0.5")).await.unwrap();
    assert_eq!(hit.reason, RevokeReason::MaliciousCrate);
    assert!(index.revocation(&pms, &id("2.1.0")).await.is_none());
    assert!(index.revocation(&pms, &id("not-semver")).await.is_none());

    // A payload that cannot be found cannot match a digest revocation, but a
    // version revocation still applies.
    let pms = package_at(Some(&tmp.path().join("missing")));
    assert!(index.revocation(&pms, &id("2.1.0")).await.is_none());
    assert!(index.revocation(&pms, &id("2.0.1")).await.is_some());

    // Only cargo packages are covered.
    let pms = package_at(Some(&krate));
    let registry_plugin = PackageId::new("reg", "widget-lib", "2.0.5");
    assert!(index.revocation(&pms, &registry_plugin).await.is_none());
    // A crate nobody audited costs nothing and matches nothing.
    let unaudited = PackageId::new(CARGO_PM, "gadget", "1.0.0");
    assert!(index.revocation(&pms, &unaudited).await.is_none());
}

/// With only `versions` revocations, no digest could match, so `answer`
/// never reads the payload.
#[tokio::test]
async fn answer_skips_the_digest_when_nothing_could_match_it() {
    let tmp = tempfile::tempdir().unwrap();
    write_audits(
        tmp.path(),
        "widget-lib",
        "format = 1\n[[revoked]]\nversions = \"<1.0.0\"\nreason = \"off-task\"\n",
    );
    let index = AuditIndex::new(tmp.path());
    let pms = package_at(None);
    let id = |version: &str| PackageId::new(CARGO_PM, "widget-lib", version);
    assert_eq!(index.answer(&pms, &id("2.0.0")).await, Answer::NotAudited);
    assert!(matches!(
        index.answer(&pms, &id("0.5.0")).await,
        Answer::Revoked(_)
    ));
}
