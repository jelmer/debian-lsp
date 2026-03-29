use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared lintian tag cache, loaded from `lintian-explain-tags --list`.
pub type SharedLintianTagCache = Arc<RwLock<LintianTagCache>>;

/// Cache of known lintian tags with their one-line descriptions.
pub struct LintianTagCache {
    /// (tag_name, visibility) pairs, populated lazily.
    tags: Option<Vec<(String, String)>>,
}

impl LintianTagCache {
    pub fn new() -> Self {
        Self { tags: None }
    }

    /// Return the cached tags, loading them on first call.
    pub async fn get_tags(&mut self) -> &[(String, String)] {
        if self.tags.is_none() {
            self.tags = Some(load_tags().await);
        }
        self.tags.as_deref().unwrap_or(&[])
    }
}

/// Load all known lintian tags by running `lintian-explain-tags --list`.
async fn load_tags() -> Vec<(String, String)> {
    let output = match tokio::process::Command::new("lintian-explain-tags")
        .arg("--list")
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| (line.to_string(), String::new()))
        .collect()
}
