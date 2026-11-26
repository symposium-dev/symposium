//! Distinct Git subpaths must retain their own cached content.

use symposium_install::UpdateLevel;
use symposium_install::git::{CACHE_META_FILENAME, GitCacheManager};

const NESTED_URL: &str = "https://github.com/example/skills/tree/main/a/b";
const FLAT_URL: &str = "https://github.com/example/skills/tree/main/a--b";

#[test]
fn all_source_fields_participate_in_cache_identity() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let urls = [
        NESTED_URL,
        FLAT_URL,
        "https://github.com/another/skills/tree/main/a/b",
        "https://github.com/example/tools/tree/main/a/b",
        "https://github.com/example/skills/tree/release/a/b",
        // Formerly this ref/subpath split collided with main + a/b.
        "https://github.com/example/skills/tree/main--a/b",
        "https://github.com/example/skills",
    ];
    let mut paths = std::collections::HashSet::new();
    for url in urls {
        assert!(
            paths.insert(cache.cache_path_for_url(url).unwrap()),
            "duplicate cache identity for {url}"
        );
    }
}

#[test]
fn equivalent_url_spellings_share_a_cache_entry() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    assert_eq!(
        cache.cache_path_for_url(NESTED_URL),
        cache.cache_path_for_url("https://GITHUB.COM/example/skills/tree/main/a/b/"),
    );
}

#[test]
fn persisted_cache_key_format_is_stable() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let path = cache.cache_path_for_url(NESTED_URL).unwrap();
    // SHA-256 of the length-prefixed identity: 7:example6:skills4:main3:a/b.
    assert_eq!(
        path.file_name().unwrap(),
        "github-v1-d68754ab83c96f8d580f4cfeb3fb4cfd83a29529a859ae3e91ec5c6c08ba6e9b"
    );
}

#[tokio::test]
async fn non_ascii_subpaths_use_utf8_byte_lengths_and_retain_their_content() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let url = "https://github.com/example/skills/tree/main/skills/编程";
    let path = cache.cache_path_for_url(url).unwrap();
    // skills/编程 occupies 13 UTF-8 bytes, rather than 9 Unicode scalar values.
    // Freeze SHA-256 of 7:example6:skills4:main13:skills/编程.
    assert_eq!(
        path.file_name().unwrap(),
        "github-v1-b413a1ecc1bcd397758e308a57a16efc5686893d6d61894a9bf8e0141f1c9d0c"
    );
    seed_cached_skill(&cache, url, "编程 skill");
    assert_eq!(cached_skill(&cache, url).await, "编程 skill");
}

#[tokio::test]
async fn case_sensitive_subpaths_keep_their_own_cached_content() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let upper = "https://github.com/example/skills/tree/main/Skill";
    let lower = "https://github.com/example/skills/tree/main/skill";
    seed_cached_skill(&cache, upper, "uppercase path");
    seed_cached_skill(&cache, lower, "lowercase path");
    assert_eq!(cached_skill(&cache, upper).await, "uppercase path");
    assert_eq!(cached_skill(&cache, lower).await, "lowercase path");
}

#[test]
fn long_and_windows_sensitive_subpaths_produce_one_bounded_filename() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let url = format!(
        "https://github.com/example/skills/tree/main/{}CON/notes:2026/..\\outside/skill.",
        "long/".repeat(200)
    );
    let path = cache.cache_path_for_url(&url).unwrap();
    let relative = path.strip_prefix(root.path()).unwrap();
    assert_eq!(relative.components().count(), 1);
    let filename = relative.to_str().unwrap();
    assert!(filename.len() <= 255);
    assert!(
        filename
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    );
    // Exercise the real filesystem, including Windows filename rules.
    std::fs::create_dir(&path).unwrap();
}

#[tokio::test]
async fn new_cache_entries_are_selected_and_legacy_contents_remain_untouched() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());
    let legacy = root.path().join("example--skills@main--a--b");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("SKILL.md"), "ambiguous legacy content").unwrap();

    for url in [NESTED_URL, FLAT_URL] {
        assert!(
            !cache.cache_path_for_url(url).unwrap().exists(),
            "new-key lookup must miss legacy-only content for {url}"
        );
    }

    seed_cached_skill(&cache, NESTED_URL, "nested skill");
    seed_cached_skill(&cache, FLAT_URL, "flat skill");
    assert_eq!(cached_skill(&cache, NESTED_URL).await, "nested skill");
    assert_eq!(cached_skill(&cache, FLAT_URL).await, "flat skill");
    assert_eq!(
        std::fs::read_to_string(legacy.join("SKILL.md")).unwrap(),
        "ambiguous legacy content"
    );
}

#[tokio::test]
async fn distinct_git_subpaths_keep_their_own_cached_content() {
    let root = tempfile::tempdir().unwrap();
    let cache = GitCacheManager::from_cache_dir(root.path());

    seed_cached_skill(&cache, NESTED_URL, "nested skill");
    assert_eq!(cached_skill(&cache, NESTED_URL).await, "nested skill");

    seed_cached_skill(&cache, FLAT_URL, "flat skill");
    assert_eq!(cached_skill(&cache, FLAT_URL).await, "flat skill");

    assert_eq!(
        cached_skill(&cache, NESTED_URL).await,
        "nested skill",
        "caching a--b must not replace the content returned for a/b"
    );
}

/// Populate fresh cache entries so acquisitions need no GitHub requests.
fn seed_cached_skill(cache: &GitCacheManager, url: &str, content: &str) {
    let directory = cache.cache_path_for_url(url).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("SKILL.md"), content).unwrap();
    let metadata = serde_json::json!({
        "commit_sha": "0123456789abcdef0123456789abcdef01234567",
        "fetched_at": chrono::Utc::now().to_rfc3339(),
        "source_url": url,
    });
    std::fs::write(
        directory.join(CACHE_META_FILENAME),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
}

async fn cached_skill(cache: &GitCacheManager, url: &str) -> String {
    let directory = cache.fetch_url(url, UpdateLevel::None).await.unwrap();
    std::fs::read_to_string(directory.join("SKILL.md")).unwrap()
}
