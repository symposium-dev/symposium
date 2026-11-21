//! Distinct Git subpaths must retain their own cached content.

use symposium_install::UpdateLevel;
use symposium_install::git::{CACHE_META_FILENAME, GitCacheManager};

const NESTED_URL: &str = "https://github.com/example/skills/tree/main/a/b";
const FLAT_URL: &str = "https://github.com/example/skills/tree/main/a--b";

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
