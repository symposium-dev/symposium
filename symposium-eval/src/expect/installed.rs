//! Whether sync installed a task's skills (RFD reference, "Skills installed").
//!
//! Sync copies a skill's source directory into the client's skill directory, under the
//! skill's name or, when two sources claim it, `<name>-<hash>`, and marks the copy with
//! `.symposium` and a `.gitignore`. A skill is installed when such a marked copy holds
//! what its source in `evals/` does, read as sync reads a tree: links followed, and a
//! broken link, a link back into a directory being read, or anything neither file nor
//! directory left out. This walk is the checker's own, so it does not share a mistake
//! with the copy it checks.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::{Verdict, file};
use crate::task::{SkillExpectations, SkillName};

/// The marker sync leaves in every directory it installs.
const MARKER: &str = ".symposium";
/// The files sync adds, which are not skill content.
const MANAGED: [&str; 2] = [MARKER, ".gitignore"];

/// Larger than any `SKILL.md` a hashed copy is identified by.
const MAX_SKILL_MD: u64 = 8 * 1024 * 1024;

/// Which of the two install assertions a skill is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallAssertion {
    Installed,
    NotInstalled,
}

/// What the skill directory showed for one skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(crate) enum InstallEvidence {
    /// No copy sync marked for the skill.
    Absent,
    /// A marked copy holding what the skill's source does.
    Matching,
    /// Marked copies, none holding what the source does.
    Differing,
    /// A marked copy whose contents did not need comparing: any copy fails
    /// `not_installed`.
    Present,
    /// The skill directory, or a copy in it, could not be read.
    Unreadable,
    /// A `<name>-<hash>` directory whose `SKILL.md` cannot say whose copy it is, and no
    /// other copy that settles the assertion.
    Unidentified,
    /// None of the task's plugins provides the skill, so nothing says what it holds.
    NoSource,
}

/// One skill's install assertion, decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillInstall {
    pub(crate) skill: SkillName,
    pub(crate) assertion: InstallAssertion,
    pub(crate) evidence: InstallEvidence,
    pub(crate) verdict: Verdict,
}

impl InstallAssertion {
    fn decide(self, evidence: InstallEvidence) -> Verdict {
        use InstallEvidence::{
            Absent, Differing, Matching, NoSource, Present, Unidentified, Unreadable,
        };
        match (self, evidence) {
            (Self::Installed, Matching) | (Self::NotInstalled, Absent) => Verdict::Pass,
            (Self::Installed, Absent | Differing)
            | (Self::NotInstalled, Matching | Differing | Present) => Verdict::Fail,
            (Self::Installed, Present | NoSource | Unreadable | Unidentified)
            | (Self::NotInstalled, NoSource | Unreadable | Unidentified) => Verdict::Inconclusive,
        }
    }
}

/// Decides `expected`'s `installed` and `not_installed` lists against `skills`, the
/// client's skill directory, with `source_of` giving each skill's directory in
/// `evals/`.
pub(crate) fn skill_installs(
    skills: &Path,
    expected: &SkillExpectations,
    source_of: impl Fn(&SkillName) -> Option<PathBuf>,
) -> Vec<SkillInstall> {
    let lists = [
        (InstallAssertion::Installed, expected.installed()),
        (InstallAssertion::NotInstalled, expected.not_installed()),
    ];
    lists
        .into_iter()
        .flat_map(|(assertion, names)| names.iter().map(move |skill| (assertion, skill)))
        .map(|(assertion, skill)| {
            let evidence = match (assertion, source_of(skill)) {
                (InstallAssertion::Installed, None) => InstallEvidence::NoSource,
                (InstallAssertion::Installed, Some(source)) => installed(skills, skill, &source),
                (InstallAssertion::NotInstalled, _) => presence(skills, skill),
            };
            SkillInstall {
                skill: skill.clone(),
                assertion,
                evidence,
                verdict: assertion.decide(evidence),
            }
        })
        .collect()
}

/// What `skills` shows for `skill`, compared with its `source`: one matching copy
/// settles it, whatever else is there.
fn installed(skills: &Path, skill: &SkillName, source: &Path) -> InstallEvidence {
    let Ok(candidates) = candidates(skills, skill) else {
        return InstallEvidence::Unreadable;
    };
    let mut unreadable = candidates.unreadable;
    for copy in &candidates.copies {
        match same_contents(copy, source) {
            Ok(true) => return InstallEvidence::Matching,
            Ok(false) => {}
            Err(_) => unreadable = true,
        }
    }
    if unreadable {
        InstallEvidence::Unreadable
    } else if candidates.unidentified {
        InstallEvidence::Unidentified
    } else if candidates.copies.is_empty() {
        InstallEvidence::Absent
    } else {
        InstallEvidence::Differing
    }
}

/// Whether `skills` holds any copy of `skill`: one settles it, whatever else is there.
fn presence(skills: &Path, skill: &SkillName) -> InstallEvidence {
    match candidates(skills, skill) {
        Ok(candidates) if !candidates.copies.is_empty() => InstallEvidence::Present,
        Ok(candidates) if candidates.unreadable => InstallEvidence::Unreadable,
        Ok(candidates) if candidates.unidentified => InstallEvidence::Unidentified,
        Ok(_) => InstallEvidence::Absent,
        Err(_) => InstallEvidence::Unreadable,
    }
}

/// The marked directories under `skills` that could be copies of one skill.
#[derive(Debug, Default)]
struct Candidates {
    /// Directories that are copies of it.
    copies: Vec<PathBuf>,
    /// Whether a `<name>-<hash>` directory could not say whose copy it is.
    unidentified: bool,
    /// Whether an entry that could be a copy could not be inspected.
    unreadable: bool,
}

/// What `skills` holds for `skill`. A missing `skills` holds nothing. An entry that
/// cannot be inspected is noted and passed over, so it cannot hide a copy that settles.
fn candidates(skills: &Path, skill: &SkillName) -> io::Result<Candidates> {
    let entries = match fs::read_dir(skills) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Candidates::default());
        }
        Err(error) => return Err(error),
    };
    let mut candidates = Candidates::default();
    for entry in entries {
        match candidate(entry, skill) {
            Ok(Some((path, Identity::Expected))) => candidates.copies.push(path),
            Ok(Some((_, Identity::Unknown))) => candidates.unidentified = true,
            Ok(Some((_, Identity::Other)) | None) => {}
            Err(_) => candidates.unreadable = true,
        }
    }
    candidates.copies.sort();
    Ok(candidates)
}

/// The directory `entry` is and whose copy, if it is one of sync's copies that could be
/// `skill`'s.
fn candidate(
    entry: io::Result<fs::DirEntry>,
    skill: &SkillName,
) -> io::Result<Option<(PathBuf, Identity)>> {
    let entry = entry?;
    let Some(slot) = entry
        .file_name()
        .to_str()
        .and_then(|name| slot(name, skill))
    else {
        return Ok(None);
    };
    // Sync installs a real directory; a link there is not one of its copies.
    if !entry.file_type()?.is_dir() || !marked(&entry.path())? {
        return Ok(None);
    }
    let identity = match slot {
        Slot::Plain => Identity::Expected,
        Slot::Hashed => identity(&entry.path(), skill)?,
    };
    Ok(Some((entry.path(), identity)))
}

/// How a directory's name could hold a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Plain,
    /// `<skill>-<8 lowercase hex>`, which may also be another skill's own name.
    Hashed,
}

fn slot(name: &str, skill: &SkillName) -> Option<Slot> {
    match name.strip_prefix(skill.as_str())? {
        "" => Some(Slot::Plain),
        rest => {
            let hash = rest.strip_prefix('-')?;
            let lower_hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
            (hash.len() == 8 && hash.bytes().all(lower_hex)).then_some(Slot::Hashed)
        }
    }
}

/// Whether `dir` holds sync's marker, by sync's own test that it exists.
fn marked(dir: &Path) -> io::Result<bool> {
    match fs::metadata(dir.join(MARKER)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whose copy a `<name>-<hash>` directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Identity {
    Expected,
    /// Another skill's, whose own name looks hashed.
    Other,
    /// Its `SKILL.md` is missing or names no skill, so nothing says.
    Unknown,
}

/// Whose copy `dir` is, by the name in its `SKILL.md`, which sync names a copy after.
/// The agent can write there, so the file is opened as evidence is, which a pipe cannot
/// block.
fn identity(dir: &Path, skill: &SkillName) -> io::Result<Identity> {
    let Some(skill_md) = file::open(&dir.join("SKILL.md"), MAX_SKILL_MD)? else {
        return Ok(Identity::Unknown);
    };
    let mut text = String::new();
    // Bounded again, in case the file grew.
    skill_md.take(MAX_SKILL_MD + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_SKILL_MD {
        return Err(io::Error::other("larger than any SKILL.md"));
    }
    Ok(match frontmatter_name(&text) {
        Some(name) if name == skill.as_str() => Identity::Expected,
        Some(_) => Identity::Other,
        None => Identity::Unknown,
    })
}

/// The `name` of a `SKILL.md`, read as Symposium reads it: the YAML between the
/// opening `---` and the next line starting `---`, with surrounding double quotes
/// removed. `None` when there is no such name.
fn frontmatter_name(text: &str) -> Option<String> {
    let body = text.trim_start().strip_prefix("---")?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    let yaml = &body[..body.find("\n---")?];
    let frontmatter: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).ok()?;
    let name = frontmatter.get("name")?.as_str()?;
    let unquoted = name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'));
    Some(unquoted.unwrap_or(name).to_owned())
}

/// A directory or a file, the only entries sync reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dir,
    /// With its size, so files of different sizes differ unread.
    File(u64),
}

/// Every directory and file under `root` as sync reads it, as `/`-joined paths in name
/// order, without sync's own files at the top. Read after the trial's processes stop, so
/// nothing changes the tree meanwhile.
fn resolved(root: &Path) -> io::Result<Vec<(String, Kind)>> {
    let mut entries = Vec::new();
    let mut ancestors = vec![fs::canonicalize(root)?];
    resolve_into(root, "", &mut ancestors, &mut entries)?;
    entries.retain(|(path, _)| !MANAGED.contains(&path.as_str()));
    Ok(entries)
}

fn resolve_into(
    dir: &Path,
    prefix: &str,
    ancestors: &mut Vec<PathBuf>,
    entries: &mut Vec<(String, Kind)>,
) -> io::Result<()> {
    let mut children = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let name = child.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        // `fs::metadata` follows a link; one that leads nowhere is skipped.
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) if child.file_type()?.is_symlink() => continue,
            Err(error) => return Err(error),
        };
        if metadata.is_dir() {
            let canonical = fs::canonicalize(&path)?;
            if ancestors.contains(&canonical) {
                continue;
            }
            entries.push((relative.clone(), Kind::Dir));
            ancestors.push(canonical);
            resolve_into(&path, &relative, ancestors, entries)?;
            ancestors.pop();
        } else if metadata.is_file() {
            entries.push((relative, Kind::File(metadata.len())));
        }
    }
    Ok(())
}

/// Whether `copy` holds what `source` does, read as sync reads them.
fn same_contents(copy: &Path, source: &Path) -> io::Result<bool> {
    let copied = resolved(copy)?;
    if copied != resolved(source)? {
        return Ok(false);
    }
    for (path, kind) in &copied {
        if let Kind::File(len) = *kind
            && !same_bytes(&copy.join(path), &source.join(path), len)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether two files of `len` bytes hold the same bytes, read a chunk at a time, since
/// the agent chose the copy's size.
fn same_bytes(copy: &Path, source: &Path, len: u64) -> io::Result<bool> {
    // One byte past `len` shows a file that grew.
    let mut copy = File::open(copy)?.take(len + 1);
    let mut source = File::open(source)?.take(len + 1);
    let (mut copied, mut original) = ([0; 8192], [0; 8192]);
    loop {
        let read = fill(&mut copy, &mut copied)?;
        let expected = fill(&mut source, &mut original)?;
        if copied[..read] != original[..expected] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

/// Reads into `buffer` until it is full or `reader` ends, so two readers of the same
/// bytes fill their buffers alike.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn skill(name: &str) -> SkillName {
        SkillName::new(name).unwrap()
    }

    /// A skill's source, as a plugin in `evals/` holds it, with no links.
    fn source(root: &Path) -> PathBuf {
        let source = root.join("source");
        fs::create_dir_all(source.join("references")).unwrap();
        fs::write(source.join("SKILL.md"), "---\nname: first-skill\n---\n").unwrap();
        fs::write(source.join("references/notes.md"), "notes").unwrap();
        source
    }

    /// Writes `files` under `skills/dir` and marks it as sync does.
    fn copy_of(skills: &Path, dir: &str, files: &[(&str, &str)]) -> PathBuf {
        let copy = skills.join(dir);
        for (path, contents) in files {
            let to = copy.join(path);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::write(to, contents).unwrap();
        }
        fs::write(copy.join(MARKER), "").unwrap();
        fs::write(copy.join(".gitignore"), "*\n").unwrap();
        copy
    }

    /// The copy sync makes of [`source`].
    fn install(skills: &Path, dir: &str) -> PathBuf {
        copy_of(
            skills,
            dir,
            &[
                ("SKILL.md", "---\nname: first-skill\n---\n"),
                ("references/notes.md", "notes"),
            ],
        )
    }

    fn decide(
        skills: &Path,
        installed: &[&str],
        not_installed: &[&str],
        source: &Path,
    ) -> Vec<(InstallEvidence, Verdict)> {
        let expected = SkillExpectations::installing(
            installed.iter().map(|name| skill(name)).collect(),
            not_installed.iter().map(|name| skill(name)).collect(),
        );
        skill_installs(skills, &expected, |name| {
            (*name == skill("first-skill")).then(|| source.to_owned())
        })
        .into_iter()
        .map(|install| (install.evidence, install.verdict))
        .collect()
    }

    #[test]
    fn the_verdict_follows_from_the_evidence() {
        use InstallEvidence::{
            Absent, Differing, Matching, NoSource, Present, Unidentified, Unreadable,
        };
        let all = [
            Absent,
            Matching,
            Differing,
            Present,
            Unreadable,
            NoSource,
            Unidentified,
        ];

        let installed = all.map(|evidence| InstallAssertion::Installed.decide(evidence));
        let not_installed = all.map(|evidence| InstallAssertion::NotInstalled.decide(evidence));

        use Verdict::{Fail, Inconclusive, Pass};
        assert_eq!(
            installed,
            [
                Fail,
                Pass,
                Fail,
                Inconclusive,
                Inconclusive,
                Inconclusive,
                Inconclusive
            ]
        );
        assert_eq!(
            not_installed,
            [
                Pass,
                Fail,
                Fail,
                Fail,
                Inconclusive,
                Inconclusive,
                Inconclusive
            ]
        );
    }

    #[test]
    fn a_marked_copy_of_the_source_is_installed() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "first-skill");

        assert_eq!(
            decide(
                &skills,
                &["first-skill"],
                &["first-skill"],
                &source(root.path())
            ),
            [
                (InstallEvidence::Matching, Verdict::Pass),
                (InstallEvidence::Present, Verdict::Fail)
            ]
        );
    }

    #[test]
    fn a_copy_under_its_hashed_name_is_installed() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "first-skill-0123abcd");

        assert_eq!(
            decide(&skills, &["first-skill"], &[], &source(root.path())),
            [(InstallEvidence::Matching, Verdict::Pass)]
        );
    }

    #[test]
    fn a_missing_skill_directory_installs_nothing() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");

        assert_eq!(
            decide(
                &skills,
                &["first-skill"],
                &["first-skill"],
                &source(root.path())
            ),
            [
                (InstallEvidence::Absent, Verdict::Fail),
                (InstallEvidence::Absent, Verdict::Pass)
            ]
        );
    }

    #[test]
    fn an_unmarked_copy_is_not_symposiums() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        let copy = install(&skills, "first-skill");
        fs::remove_file(copy.join(MARKER)).unwrap();

        assert_eq!(
            decide(&skills, &["first-skill"], &[], &source(root.path())),
            [(InstallEvidence::Absent, Verdict::Fail)]
        );
    }

    #[test]
    fn a_copy_whose_contents_differ_is_not_installed() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        let copy = install(&skills, "first-skill");
        fs::write(copy.join("references/notes.md"), "edited").unwrap();

        assert_eq!(
            decide(&skills, &["first-skill"], &[], &source(root.path())),
            [(InstallEvidence::Differing, Verdict::Fail)]
        );
    }

    #[test]
    fn files_are_compared_past_the_first_chunk() {
        let root = TempDir::new().unwrap();
        let (copy, source) = (root.path().join("copy"), root.path().join("source"));
        let long = "x".repeat(20_000);
        fs::write(&source, &long).unwrap();
        fs::write(&copy, &long).unwrap();
        let len = long.len() as u64;

        assert!(same_bytes(&copy, &source, len).unwrap());
        fs::write(&copy, format!("{}y", &long[1..])).unwrap();
        assert!(!same_bytes(&copy, &source, len).unwrap());
        // A file larger than its listed size grew after it was listed.
        fs::write(&copy, format!("{long}y")).unwrap();
        assert!(!same_bytes(&copy, &source, len).unwrap());
    }

    #[test]
    fn a_copy_with_an_extra_file_is_not_installed() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        let copy = install(&skills, "first-skill");
        fs::write(copy.join("added.md"), "").unwrap();

        assert_eq!(
            decide(&skills, &["first-skill"], &[], &source(root.path())),
            [(InstallEvidence::Differing, Verdict::Fail)]
        );
    }

    #[test]
    fn a_skill_named_only_alike_is_another_skill() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "first-skill-extra");
        install(&skills, "first-skill0123abcd");
        install(&skills, "first-skill-0123ABCD");

        assert_eq!(
            decide(&skills, &[], &["first-skill"], &source(root.path())),
            [(InstallEvidence::Absent, Verdict::Pass)]
        );
    }

    #[test]
    fn a_skill_whose_own_name_looks_hashed_is_another_skill() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        copy_of(
            &skills,
            "first-skill-0123abcd",
            &[("SKILL.md", "---\nname: \"first-skill-0123abcd\"\n---\n")],
        );

        assert_eq!(
            decide(&skills, &[], &["first-skill"], &source(root.path())),
            [(InstallEvidence::Absent, Verdict::Pass)]
        );
    }

    #[test]
    fn a_skill_without_a_source_cannot_be_decided_installed() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "second-skill");

        assert_eq!(
            decide(
                &skills,
                &["second-skill"],
                &["second-skill"],
                &source(root.path())
            ),
            [
                (InstallEvidence::NoSource, Verdict::Inconclusive),
                (InstallEvidence::Present, Verdict::Fail)
            ]
        );
    }

    #[test]
    fn a_frontmatter_name_is_read_as_yaml() {
        for text in [
            "---\nname: x\n---\n",
            "---\nname: \"x\"\n---\n",
            "---\nname: 'x'\n---\n",
            "---\nname: x # explanatory comment\n---\n",
            "---\ndescription: |\n  two\n  lines\nname: >-\n  x\n---\n",
            "\n---\r\nname: x\r\n---\r\n",
        ] {
            assert_eq!(frontmatter_name(text).as_deref(), Some("x"), "{text:?}");
        }
    }

    #[test]
    fn a_frontmatter_without_a_name_names_nothing() {
        for text in [
            "name: x\n",
            "---\nname: x\n",
            "---\nname: [x\n---\n",
            "---\nname: [x]\n---\n",
            "---\ndescription: d\n---\n",
        ] {
            assert_eq!(frontmatter_name(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_hashed_copy_whose_name_has_a_comment_is_the_skills() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        copy_of(
            &skills,
            "first-skill-0123abcd",
            &[("SKILL.md", "---\nname: first-skill # the fixture\n---\n")],
        );

        assert_eq!(
            decide(&skills, &[], &["first-skill"], &source(root.path())),
            [(InstallEvidence::Present, Verdict::Fail)]
        );
    }

    #[test]
    fn a_hashed_copy_that_cannot_say_whose_it_is_is_inconclusive() {
        for files in [
            &[("notes.md", "")][..],
            &[("SKILL.md", "---\nname: [first-skill\n---\n")][..],
        ] {
            let root = TempDir::new().unwrap();
            let skills = root.path().join("skills");
            copy_of(&skills, "first-skill-0123abcd", files);

            assert_eq!(
                decide(
                    &skills,
                    &["first-skill"],
                    &["first-skill"],
                    &source(root.path())
                ),
                [
                    (InstallEvidence::Unidentified, Verdict::Inconclusive),
                    (InstallEvidence::Unidentified, Verdict::Inconclusive)
                ],
                "{files:?}"
            );
        }
    }

    #[test]
    fn a_known_copy_settles_what_an_unidentified_one_cannot() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "first-skill");
        copy_of(&skills, "first-skill-0123abcd", &[("notes.md", "")]);

        assert_eq!(
            decide(
                &skills,
                &["first-skill"],
                &["first-skill"],
                &source(root.path())
            ),
            [
                (InstallEvidence::Matching, Verdict::Pass),
                (InstallEvidence::Present, Verdict::Fail)
            ]
        );
    }

    #[test]
    fn a_known_copy_settles_what_an_unreadable_one_cannot() {
        let root = TempDir::new().unwrap();
        let skills = root.path().join("skills");
        install(&skills, "first-skill");
        let unreadable = copy_of(&skills, "first-skill-0123abcd", &[("notes.md", "")]);
        fs::create_dir(unreadable.join("SKILL.md")).unwrap();

        assert_eq!(
            decide(
                &skills,
                &["first-skill"],
                &["first-skill"],
                &source(root.path())
            ),
            [
                (InstallEvidence::Matching, Verdict::Pass),
                (InstallEvidence::Present, Verdict::Fail)
            ]
        );
    }

    /// Links need developer mode or elevation on Windows.
    #[cfg(unix)]
    mod links {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::process::{Child, Command};
        use std::time::{Duration, Instant};

        use super::*;

        #[test]
        fn a_named_pipe_in_a_hashed_copy_does_not_block_inspection() {
            const CHILD_SKILLS: &str = "SYMPOSIUM_EVAL_INSTALLED_FIFO_SKILLS";
            if let Some(skills) = std::env::var_os(CHILD_SKILLS) {
                assert_eq!(
                    presence(Path::new(&skills), &skill("first-skill")),
                    InstallEvidence::Unreadable
                );
                return;
            }

            let root = TempDir::new().unwrap();
            let skills = root.path().join("skills");
            let copy = copy_of(&skills, "first-skill-0123abcd", &[("notes.md", "")]);
            let pipe = copy.join("SKILL.md");
            let name = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
            // SAFETY: `name` is a valid, NUL-terminated path.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);

            // Isolate a blocking open so the watchdog can always stop and reap it,
            // including when another assertion in this test fails.
            struct Reaped {
                child: Child,
                reaped: bool,
            }
            impl Drop for Reaped {
                fn drop(&mut self) {
                    if !self.reaped {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                    }
                }
            }
            let mut child = Reaped {
                child: Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "expect::installed::tests::links::a_named_pipe_in_a_hashed_copy_does_not_block_inspection",
                        "--nocapture",
                    ])
                    .env(CHILD_SKILLS, &skills)
                    .spawn()
                    .unwrap(),
                reaped: false,
            };
            let deadline = Instant::now() + Duration::from_secs(1);
            let status = loop {
                if let Some(status) = child.child.try_wait().unwrap() {
                    child.reaped = true;
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let status =
                status.expect("inspecting an agent-created FIFO must not hang the checker");
            assert!(status.success(), "inspection subprocess failed: {status}");
        }

        #[test]
        fn a_linked_file_installs_as_the_file() {
            let root = TempDir::new().unwrap();
            let source = source(root.path());
            fs::write(root.path().join("shared.md"), "shared").unwrap();
            symlink(
                root.path().join("shared.md"),
                source.join("references/guide.md"),
            )
            .unwrap();
            let skills = root.path().join("skills");
            let copy = install(&skills, "first-skill");
            fs::write(copy.join("references/guide.md"), "shared").unwrap();

            assert_eq!(
                decide(&skills, &["first-skill"], &[], &source),
                [(InstallEvidence::Matching, Verdict::Pass)]
            );
        }

        #[test]
        fn a_linked_directory_installs_as_its_contents() {
            let root = TempDir::new().unwrap();
            let source = source(root.path());
            fs::create_dir(root.path().join("shared")).unwrap();
            fs::write(root.path().join("shared/extra.md"), "extra").unwrap();
            symlink(root.path().join("shared"), source.join("shared")).unwrap();
            let skills = root.path().join("skills");
            let copy = install(&skills, "first-skill");
            fs::create_dir(copy.join("shared")).unwrap();
            fs::write(copy.join("shared/extra.md"), "extra").unwrap();

            assert_eq!(
                decide(&skills, &["first-skill"], &[], &source),
                [(InstallEvidence::Matching, Verdict::Pass)]
            );
        }

        #[test]
        fn a_broken_link_and_a_link_to_an_ancestor_install_nothing() {
            let root = TempDir::new().unwrap();
            let source = source(root.path());
            symlink(root.path().join("absent.md"), source.join("broken.md")).unwrap();
            symlink(&source, source.join("references/loop")).unwrap();
            let skills = root.path().join("skills");
            install(&skills, "first-skill");

            assert_eq!(
                decide(&skills, &["first-skill"], &[], &source),
                [(InstallEvidence::Matching, Verdict::Pass)]
            );
        }

        #[test]
        fn a_link_in_the_skill_directory_is_not_a_copy() {
            let root = TempDir::new().unwrap();
            let skills = root.path().join("skills");
            let elsewhere = install(&root.path().join("elsewhere"), "first-skill");
            fs::create_dir_all(&skills).unwrap();
            symlink(&elsewhere, skills.join("first-skill")).unwrap();

            assert_eq!(
                decide(&skills, &[], &["first-skill"], &source(root.path())),
                [(InstallEvidence::Absent, Verdict::Pass)]
            );
        }

        #[test]
        fn a_copy_that_cannot_be_inspected_cannot_pass_not_installed() {
            let root = TempDir::new().unwrap();
            let skills = root.path().join("skills");
            let copy = install(&skills, "first-skill");
            fs::set_permissions(&copy, fs::Permissions::from_mode(0o000)).unwrap();
            // A privileged user reads it anyway, so there is nothing to test.
            let inspectable = fs::metadata(copy.join(MARKER)).is_ok();
            let decided = decide(
                &skills,
                &["first-skill"],
                &["first-skill"],
                &source(root.path()),
            );
            fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).unwrap();
            if inspectable {
                return;
            }

            assert_eq!(
                decided,
                [
                    (InstallEvidence::Unreadable, Verdict::Inconclusive),
                    (InstallEvidence::Unreadable, Verdict::Inconclusive)
                ]
            );
        }
    }
}
