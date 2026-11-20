use super::*;
use crate::expect::test_support::NativeBundle;
use tempfile::TempDir;

const OWNED: [&str; 3] = [
    ".claude/skills/a-plugin",
    ".symposium/plugins/a-plugin",
    ".symposium/.gitignore",
];

struct Fixture {
    dir: TempDir,
    project: PathBuf,
    sources: BTreeMap<String, PathBuf>,
    bundles: [NativeBundle; 2],
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        let bundles = [".claude/skills", ".symposium/plugins"]
            .map(|root| NativeBundle::new(&project.join(root).join("a-plugin")));
        fs::write(project.join(".symposium/.gitignore"), GITIGNORE_CONTENTS).unwrap();
        let mut fixture = Self {
            dir,
            project,
            sources: BTreeMap::new(),
            bundles,
        };
        fixture.add_skill("first-skill");
        fixture
    }

    fn add_skill(&mut self, name: &str) {
        let source = self.dir.path().join("sources").join(name);
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {name}\n---\nskill\n"),
        )
        .unwrap();
        for bundle in &self.bundles {
            bundle.install_skill(name, &source);
        }
        self.sources.insert(name.to_owned(), source);
    }

    fn owned(&self) -> Vec<String> {
        symposium_bundles(&self.project, &|name| {
            self.sources.get(name.as_str()).cloned()
        })
    }

    fn write_delivered(&self, path: &str, contents: &str) {
        fs::write(self.bundles[0].path().join(path), contents).unwrap();
    }

    fn edit_both_payloads(&self, contents: &str) {
        for bundle in &self.bundles {
            fs::write(bundle.path().join("skills/first-skill/SKILL.md"), contents).unwrap();
        }
    }
}

#[test]
fn a_single_skill_plugin_is_runner_owned() {
    let fixture = Fixture::new();

    assert_eq!(fixture.owned(), OWNED);
}

#[test]
fn a_bundle_can_contain_multiple_specialized_skills() {
    let mut fixture = Fixture::new();
    fixture.add_skill("second-skill");

    assert_eq!(fixture.owned(), OWNED);
}

#[test]
fn an_unknown_payload_prevents_claiming_the_whole_bundle() {
    let mut fixture = Fixture::new();
    fixture.add_skill("second-skill");
    fixture.sources.remove("second-skill");

    assert!(fixture.owned().is_empty());
}

#[test]
fn matching_edits_to_the_bundle_and_cache_do_not_authenticate_each_other() {
    let fixture = Fixture::new();
    fixture.edit_both_payloads("edited");

    assert!(fixture.owned().is_empty());
}

#[test]
fn extra_files_and_modified_metadata_remain_agent_edits() {
    for (path, contents) in [
        ("extra", "added"),
        ("skills/first-skill/extra", "added"),
        ("plugin.json", "{}"),
        (".claude-plugin/plugin.json", "{}"),
        (".symposium", "written"),
        (".gitignore", "!extra\n"),
    ] {
        let fixture = Fixture::new();
        fixture.write_delivered(path, contents);

        assert!(
            !fixture
                .owned()
                .iter()
                .any(|path| path.starts_with(".claude/")),
            "{path}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_link_to_the_right_payload_is_not_runner_owned() {
    let fixture = Fixture::new();
    let payload = fixture.bundles[0]
        .path()
        .join("skills/first-skill/SKILL.md");
    fs::remove_file(&payload).unwrap();
    std::os::unix::fs::symlink(fixture.sources["first-skill"].join("SKILL.md"), &payload).unwrap();

    assert!(
        !fixture
            .owned()
            .iter()
            .any(|path| path.starts_with(".claude/"))
    );
}
