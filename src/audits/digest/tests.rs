use super::*;

const GOLDEN: &str = "sha256:d52ea77032bf0b88b9a0ba259bf243d0ed061103c082ba79ebb237a0b290ea28";

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, bytes).unwrap();
}

const CARGO_TOML: &[u8] = b"[package]\nname = \"golden\"\nversion = \"1.0.0\"\n\n\
    [package.metadata.symposium]\ndepends-on = [\"serde\"]\npredicates = [\"env(FOO)\"]\n";

/// The golden fixture. Every byte is written explicitly (including the CRLFs)
/// so no platform converts line endings. The expected digest was computed by
/// an independent implementation of the scheme in the module docs.
fn fixture(root: &Path) {
    write(root, "Cargo.toml", CARGO_TOML);
    write(
        root,
        "Symposium.toml",
        b"name = \"golden\"\r\n[[skills]]\r\nsource.path = \"extra\"\r\n",
    );
    write(
        root,
        "extra/one/SKILL.md",
        b"---\nname: one\ndescription: first\n---\nbody one\n",
    );
    write(root, "extra/one/resources/data.bin", &[0, 1, 2, 255]);
    write(
        root,
        "skills/two/SKILL.md",
        b"---\nname: two\ndescription: second\n---\nbody two\r\n",
    );
    write(root, "skills/two/scripts/run.sh", b"echo hi\r\n");
    // Outside the payload.
    write(root, "src/lib.rs", b"pub fn code() {}\n");
    write(root, "README.md", b"not hashed\n");
    write(root, "other/notes.txt", b"not in a skill dir\n");
}

fn digest_of(root: &Path) -> String {
    payload_digest(root).unwrap().to_string()
}

fn tmp_fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path());
    tmp
}

#[test]
fn golden_digest() {
    let tmp = tmp_fixture();
    assert_eq!(digest_of(tmp.path()), GOLDEN);
}

/// Metadata covering every TOML type, nested tables and an escaped string.
const RICH_CARGO_TOML: &[u8] = b"[package]\nname = \"rich\"\nversion = \"1.0.0\"\n\n\
    [package.metadata.symposium]\ndepends-on = [\"serde\"]\n\n\
    [package.metadata.symposium.extra]\ncount = 42\nratio = 0.1\nwhole = 100.0\nflag = true\n\
    when = 2024-01-02T03:04:05Z\nnote = \"say \\\"hi\\\"\\n\\u00e9 \\u0001 /\"\n\n\
    [package.metadata.symposium.extra.deep]\nz = \"last\"\na = [1, 2]\n";

/// Golden for [`RICH_CARGO_TOML`] plus one skill. Computed by an independent
/// python3 implementation of the scheme in the module docs: `tomllib` for the
/// table, `json.dumps(sort_keys=True, separators=(",", ":"),
/// ensure_ascii=False)` for the metadata entry, then the framing and SHA-256.
///
/// It is a vector for the canonical JSON and the framing only: the `extra`
/// table is not a valid plugin manifest, so [`payload_digest`] refuses this
/// crate, and the test hashes the two entries directly.
const RICH_GOLDEN: &str = "sha256:17971a166d5cbf065b57531ab2def02d23b4aefe9c5701f42d2cd1b09f817c2f";

#[test]
fn golden_digest_of_rich_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "Cargo.toml", RICH_CARGO_TOML);
    write(
        tmp.path(),
        "skills/one/SKILL.md",
        b"---\nname: one\ndescription: first\n---\nbody\n",
    );
    let table = symposium_pm_cargo::metadata::symposium_metadata(&tmp.path().join("Cargo.toml"))
        .unwrap()
        .unwrap();
    let json = canonical_json(&toml::Value::Table(table));
    assert_eq!(
        json,
        "{\"depends-on\":[\"serde\"],\"extra\":{\"count\":42,\"deep\":{\"a\":[1,2],\"z\":\"last\"},\"flag\":true,\"note\":\"say \\\"hi\\\"\\n\u{e9} \\u0001 /\",\"ratio\":0.1,\"when\":\"2024-01-02T03:04:05Z\",\"whole\":100.0}}"
    );
    let entries = BTreeMap::from([
        (METADATA_ENTRY.to_string(), json.into_bytes()),
        (
            "skills/one/SKILL.md".to_string(),
            std::fs::read(tmp.path().join("skills/one/SKILL.md")).unwrap(),
        ),
    ]);
    assert_eq!(hash_entries(&entries).to_string(), RICH_GOLDEN);
    assert!(matches!(
        payload_digest(tmp.path()).unwrap_err(),
        Ineligible::Manifest(_)
    ));
}

#[test]
fn a_skill_resource_changes_the_digest() {
    let tmp = tmp_fixture();
    write(tmp.path(), "extra/one/resources/data.bin", &[0, 1, 2, 254]);
    assert_ne!(digest_of(tmp.path()), GOLDEN);

    let tmp = tmp_fixture();
    write(tmp.path(), "skills/two/scripts/run.sh", b"echo hi\n");
    assert_ne!(
        digest_of(tmp.path()),
        GOLDEN,
        "line endings are not normalized"
    );

    let tmp = tmp_fixture();
    write(tmp.path(), "skills/two/resources/new.txt", b"added");
    assert_ne!(digest_of(tmp.path()), GOLDEN, "an added file counts");
}

#[test]
fn code_and_unrelated_cargo_toml_keys_do_not_change_the_digest() {
    let tmp = tmp_fixture();
    write(tmp.path(), "src/lib.rs", b"pub fn other_code() {}\n");
    write(tmp.path(), "src/new.rs", b"fn x() {}");
    write(tmp.path(), "other/notes.txt", b"changed");
    let mut cargo = CARGO_TOML.to_vec();
    cargo.extend_from_slice(b"\n[dependencies]\nserde = \"1\"\n");
    let cargo = String::from_utf8(cargo)
        .unwrap()
        .replace("version = \"1.0.0\"", "version = \"2.3.4\"");
    write(tmp.path(), "Cargo.toml", cargo.as_bytes());
    assert_eq!(digest_of(tmp.path()), GOLDEN);
}

#[test]
fn metadata_key_order_and_formatting_do_not_matter() {
    let tmp = tmp_fixture();
    write(
        tmp.path(),
        "Cargo.toml",
        b"[package]\nname = \"golden\"\nversion = \"1.0.0\"\n\n\
          [package.metadata.symposium]\n\n  predicates = [ \"env(FOO)\" ]\n\
          depends-on   = [\"serde\"]\n",
    );
    assert_eq!(digest_of(tmp.path()), GOLDEN);
}

#[test]
fn changing_the_metadata_changes_the_digest() {
    let tmp = tmp_fixture();
    write(
        tmp.path(),
        "Cargo.toml",
        b"[package]\nname = \"golden\"\n[package.metadata.symposium]\n\
          depends-on = [\"tokio\"]\npredicates = [\"env(FOO)\"]\n",
    );
    assert_ne!(digest_of(tmp.path()), GOLDEN);
}

#[test]
fn renaming_a_file_changes_the_digest_but_creation_order_does_not() {
    let tmp = tmp_fixture();
    std::fs::rename(
        tmp.path().join("skills/two/scripts/run.sh"),
        tmp.path().join("skills/two/scripts/go.sh"),
    )
    .unwrap();
    assert_ne!(digest_of(tmp.path()), GOLDEN);

    // The same files written in the opposite order hash the same.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "skills/two/scripts/run.sh", b"echo hi\r\n");
    write(
        root,
        "skills/two/SKILL.md",
        b"---\nname: two\ndescription: second\n---\nbody two\r\n",
    );
    write(root, "extra/one/resources/data.bin", &[0, 1, 2, 255]);
    write(
        root,
        "extra/one/SKILL.md",
        b"---\nname: one\ndescription: first\n---\nbody one\n",
    );
    write(
        root,
        "Symposium.toml",
        b"name = \"golden\"\r\n[[skills]]\r\nsource.path = \"extra\"\r\n",
    );
    write(root, "Cargo.toml", CARGO_TOML);
    assert_eq!(digest_of(root), GOLDEN);
}

#[test]
fn a_crate_with_no_manifest_hashes_its_default_skills_directory() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "skills/guide/SKILL.md",
        b"---\nname: guide\n---\n",
    );
    let with_skill = payload_digest(tmp.path()).unwrap();
    write(
        tmp.path(),
        "skills/guide/SKILL.md",
        b"---\nname: guide2\n---\n",
    );
    assert_ne!(payload_digest(tmp.path()).unwrap(), with_skill);

    // Opting out of the default group takes the skills out of the payload.
    write(
        tmp.path(),
        "Symposium.toml",
        b"[defaults]\nskills = false\n",
    );
    let opted_out = payload_digest(tmp.path()).unwrap();
    write(
        tmp.path(),
        "skills/guide/SKILL.md",
        b"---\nname: guide3\n---\n",
    );
    assert_eq!(payload_digest(tmp.path()).unwrap(), opted_out);
}

#[test]
fn a_file_reached_through_two_groups_counts_once() {
    let one = tempfile::tempdir().unwrap();
    write(
        one.path(),
        "Symposium.toml",
        b"[[skills]]\nsource.path = \"skills\"\n",
    );
    write(one.path(), "skills/a/SKILL.md", b"---\nname: a\n---\n");

    // `skills` is already the default group, so this overlaps it exactly.
    let two = tempfile::tempdir().unwrap();
    write(
        two.path(),
        "Symposium.toml",
        b"[[skills]]\nsource.path = \"skills\"\n[[skills]]\nsource.path = \"skills/a\"\n",
    );
    write(two.path(), "skills/a/SKILL.md", b"---\nname: a\n---\n");
    let a = payload_digest(one.path()).unwrap();
    let b = payload_digest(two.path()).unwrap();
    // The manifests differ, so the digests do; what matters is that the
    // overlapping group neither errors nor double-counts the shared file.
    assert_ne!(a, b);
    let entries = payload_entries(two.path()).unwrap();
    assert!(entries.contains_key("skills/a/SKILL.md"));
    assert_eq!(entries.len(), 2, "{:?}", entries.keys().collect::<Vec<_>>());
}

/// Every payload that cannot be pinned is refused with the reason, never
/// digested: what it fetches or reaches could change, or hide skill content,
/// under the same crate release.
#[test]
fn ineligible_payloads() {
    type Setup = fn(&Path);
    let git: Setup = |root| {
        write(
            root,
            "Symposium.toml",
            b"[[skills]]\nsource.git = \"https://github.com/example/skills\"\n",
        )
    };
    let escapes: Setup = |root| {
        write(
            root,
            "Symposium.toml",
            b"[[skills]]\nsource.path = \"../shared\"\n",
        );
        write(
            &root.join("../shared"),
            "s/SKILL.md",
            b"---\nname: s\n---\n",
        );
    };
    let oversized: Setup = |root| {
        write(
            root,
            "skills/two/big.bin",
            &vec![0u8; MAX_PAYLOAD_BYTES as usize],
        )
    };
    let unparsable: Setup = |root| write(root, "Symposium.toml", b"not = [valid");
    let invalid: Setup = |root| write(root, "Symposium.toml", b"name = 5\n");
    let invalid_metadata: Setup = |root| {
        write(
            root,
            "Cargo.toml",
            b"[package]\nname = \"x\"\nversion = \"1.0.0\"\n\
              [package.metadata.symposium]\nname = 5\n",
        )
    };
    let mut cases: Vec<(&str, Setup, Ineligible)> = vec![
        (
            "a git skill group",
            git,
            Ineligible::GitSkillSource("https://github.com/example/skills".into()),
        ),
        (
            "a skill path leaving the crate",
            escapes,
            Ineligible::EscapesCrate("../shared".into()),
        ),
        ("an oversized payload", oversized, Ineligible::TooLarge),
        // A layer that does not parse is not dropped (as loading does) to
        // digest the defaults instead.
        (
            "an unparsable manifest",
            unparsable,
            Ineligible::Manifest(String::new()),
        ),
        (
            "an invalid manifest",
            invalid,
            Ineligible::Manifest(String::new()),
        ),
        (
            "invalid metadata",
            invalid_metadata,
            Ineligible::Manifest(String::new()),
        ),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let special: Setup = |root| {
            let status = std::process::Command::new("mkfifo")
                .arg(root.join("skills/two/pipe"))
                .status()
                .unwrap();
            assert!(status.success());
        };
        let broken: Setup = |root| symlink("nowhere", root.join("skills/two/dangling")).unwrap();
        // Discovery ignores it, but it could be repointed at a directory of
        // skills the digest never saw.
        let broken_beside: Setup =
            |root| symlink("nowhere", root.join("skills/maybe-skills")).unwrap();
        let looped: Setup = |root| symlink("..", root.join("skills/two/back")).unwrap();
        cases.extend([
            (
                "a special file",
                special,
                Ineligible::Unsupported("skills/two/pipe".into()),
            ),
            (
                "a broken symlink",
                broken,
                Ineligible::BrokenLink("skills/two/dangling".into()),
            ),
            (
                "a broken symlink beside the skills",
                broken_beside,
                Ineligible::BrokenLink("skills/maybe-skills".into()),
            ),
            (
                "a directory loop",
                looped,
                Ineligible::LinkLoop("skills/two/back".into()),
            ),
        ]);
    }
    // macOS file systems refuse names that are not UTF-8.
    #[cfg(target_os = "linux")]
    {
        let non_utf8: Setup = |root| {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::OsStr::from_bytes(b"bad\xff.md");
            std::fs::write(root.join("skills/two").join(name), b"x").unwrap();
        };
        cases.push((
            "a non-UTF-8 path",
            non_utf8,
            Ineligible::Unsupported(String::new()),
        ));
    }
    for (what, setup, expected) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("crate");
        fixture(&root);
        setup(&root);
        let err = payload_digest(&root).unwrap_err();
        // An empty detail matches the variant alone: its message is the
        // parser's or the platform's.
        let matches = match (&err, &expected) {
            (Ineligible::Manifest(_), Ineligible::Manifest(d))
            | (Ineligible::Unsupported(_), Ineligible::Unsupported(d))
                if d.is_empty() =>
            {
                true
            }
            _ => err == expected,
        };
        assert!(matches, "{what}: {err:?}");
    }
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        payload_digest(&tmp.path().join("missing")).unwrap_err(),
        Ineligible::NotADirectory(_)
    ));
}

#[test]
fn canonical_json_sorts_keys_and_stringifies_datetimes() {
    let value: toml::Value = toml::from_str(
        "z = 1\na = [true, 1.5, \"x\\\"y\"]\nwhen = 2024-01-02T03:04:05Z\n[m]\nb = 1\na = 2\n",
    )
    .unwrap();
    assert_eq!(
        canonical_json(&value),
        r#"{"a":[true,1.5,"x\"y"],"m":{"a":2,"b":1},"when":"2024-01-02T03:04:05Z","z":1}"#
    );
}
