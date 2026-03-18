use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;

/// Version information for a package.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// The version string.
    pub version: String,
    /// The suites this version is available in.
    pub suites: Vec<String>,
}

/// Access to the package cache.
#[async_trait::async_trait]
pub trait PackageCache: Send + Sync {
    /// Get packages whose names start with a given prefix, sorted.
    fn get_packages_with_prefix(&self, prefix: &str) -> Vec<String>;

    /// Get the cached short description for a package.
    fn get_description(&self, package: &str) -> Option<&str>;

    /// Load and cache versions for a package.
    async fn load_versions(&mut self, package: &str) -> Option<&[VersionInfo]>;

    /// Load and cache the list of packages that provide a given virtual package.
    async fn load_providers(&mut self, package: &str) -> Option<&[String]>;

    /// Insert a package name with its short description into the cache.
    fn insert_package(&mut self, name: String, description: String);
}

/// Thread-safe shared package cache.
pub type SharedPackageCache = Arc<RwLock<dyn PackageCache>>;

/// Package cache backed by `apt-cache`.
pub struct AptPackageCache {
    /// All available package names (sorted).
    packages: Vec<String>,
    /// Package descriptions (package name -> short description).
    descriptions: HashMap<String, String>,
    /// Cached versions for specific packages.
    versions: HashMap<String, Vec<VersionInfo>>,
    /// Cached providers for virtual packages.
    providers: HashMap<String, Vec<String>>,
}

impl AptPackageCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
            descriptions: HashMap::new(),
            versions: HashMap::new(),
            providers: HashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl PackageCache for AptPackageCache {
    fn get_packages_with_prefix(&self, prefix: &str) -> Vec<String> {
        let prefix_lower = prefix.to_ascii_lowercase();
        self.packages
            .iter()
            .filter(|p| p.starts_with(&prefix_lower))
            .cloned()
            .collect()
    }

    fn get_description(&self, package: &str) -> Option<&str> {
        self.descriptions.get(package).map(|s| s.as_str())
    }

    async fn load_versions(&mut self, package: &str) -> Option<&[VersionInfo]> {
        if self.versions.contains_key(package) {
            return self.versions.get(package).map(|v| v.as_slice());
        }

        match Command::new("apt-cache")
            .arg("madison")
            .arg(package)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut version_suites: HashMap<String, Vec<String>> = HashMap::new();
                for line in text.lines() {
                    let parts: Vec<&str> = line.split('|').collect();
                    if parts.len() >= 3 {
                        let version = parts[1].trim().to_string();
                        let suite = parts[2].trim().to_string();
                        version_suites.entry(version).or_default().push(suite);
                    }
                }

                let mut versions: Vec<VersionInfo> = version_suites
                    .into_iter()
                    .map(|(version, suites)| VersionInfo { version, suites })
                    .collect();
                versions.sort_by(|a, b| b.version.cmp(&a.version));

                self.versions.insert(package.to_string(), versions);
                self.versions.get(package).map(|v| v.as_slice())
            }
            _ => None,
        }
    }

    async fn load_providers(&mut self, package: &str) -> Option<&[String]> {
        if self.providers.contains_key(package) {
            return self.providers.get(package).map(|v| v.as_slice());
        }

        match Command::new("apt-cache")
            .arg("showpkg")
            .arg(package)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut providers = Vec::new();
                let mut in_reverse_provides = false;

                for line in text.lines() {
                    if line.starts_with("Reverse Provides:") {
                        in_reverse_provides = true;
                        continue;
                    }
                    if in_reverse_provides {
                        if let Some(name) = line.split_whitespace().next() {
                            if !providers.contains(&name.to_string()) {
                                providers.push(name.to_string());
                            }
                        }
                    }
                }

                providers.sort();
                self.providers.insert(package.to_string(), providers);
                self.providers.get(package).map(|v| v.as_slice())
            }
            _ => None,
        }
    }

    fn insert_package(&mut self, name: String, description: String) {
        let pos = self.packages.binary_search(&name).unwrap_or_else(|p| p);
        self.packages.insert(pos, name.clone());
        self.descriptions.insert(name, description);
    }
}

/// Create a new shared cache backed by apt-cache.
pub fn new_shared_cache() -> SharedPackageCache {
    Arc::new(RwLock::new(AptPackageCache::new()))
}

/// Stream package names and descriptions from `apt-cache search` into the
/// shared cache.
///
/// Each line is `name - description`, inserted as soon as it is read so
/// completions are available immediately while the full list loads.
pub async fn stream_packages_into(cache: &SharedPackageCache) {
    let Ok(mut child) = Command::new("apt-cache")
        .args(["search", "--names-only", "."])
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return;
    };

    let Some(stdout) = child.stdout.take() else {
        return;
    };

    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some((name, description)) = line.split_once(" - ") {
            cache
                .write()
                .await
                .insert_package(name.to_string(), description.to_string());
        }
    }
}

#[cfg(test)]
/// Simple in-memory package cache for tests.
#[derive(Default)]
pub struct TestPackageCache {
    /// Packages and their optional descriptions.
    pub packages: Vec<(String, Option<String>)>,
    /// Cached versions.
    pub versions: HashMap<String, Vec<VersionInfo>>,
    /// Cached providers for virtual packages.
    pub providers: HashMap<String, Vec<String>>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl PackageCache for TestPackageCache {
    fn get_packages_with_prefix(&self, prefix: &str) -> Vec<String> {
        let prefix_lower = prefix.to_ascii_lowercase();
        let mut result: Vec<String> = self
            .packages
            .iter()
            .filter(|(name, _)| name.starts_with(&prefix_lower))
            .map(|(name, _)| name.clone())
            .collect();
        result.sort_unstable();
        result
    }

    fn get_description(&self, package: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|(name, _)| name == package)
            .and_then(|(_, desc)| desc.as_deref())
    }

    async fn load_versions(&mut self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    async fn load_providers(&mut self, package: &str) -> Option<&[String]> {
        self.providers.get(package).map(|v| v.as_slice())
    }

    fn insert_package(&mut self, name: String, description: String) {
        self.packages.push((name, Some(description)));
    }
}

#[cfg(test)]
impl TestPackageCache {
    /// Create a shared test cache.
    pub fn new_shared(packages: &[(&str, Option<&str>)]) -> SharedPackageCache {
        let mut cache = TestPackageCache::default();
        for &(name, desc) in packages {
            cache
                .packages
                .push((name.to_string(), desc.map(|s| s.to_string())));
        }
        Arc::new(RwLock::new(cache))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cache() {
        let cache = AptPackageCache::new();
        assert!(cache.get_packages_with_prefix("foo").is_empty());
    }

    #[test]
    fn test_insert_package_sorted() {
        let mut cache = AptPackageCache::new();
        cache.insert_package("cmake".to_string(), "cross-platform make".to_string());
        cache.insert_package("apt".to_string(), "package manager".to_string());
        cache.insert_package("zsh".to_string(), "shell".to_string());
        cache.insert_package("debhelper".to_string(), "helper tools".to_string());
        assert_eq!(cache.packages, vec!["apt", "cmake", "debhelper", "zsh"]);
        assert_eq!(cache.get_description("cmake"), Some("cross-platform make"));
    }

    #[test]
    fn test_test_cache_prefix() {
        let cache_arc = TestPackageCache::new_shared(&[
            ("debhelper", None),
            ("debhelper-compat", None),
            ("cmake", Some("cross-platform make")),
        ]);
        let cache = cache_arc.try_read().unwrap();

        let deb = cache.get_packages_with_prefix("deb");
        assert_eq!(deb, vec!["debhelper", "debhelper-compat"]);

        let cm = cache.get_packages_with_prefix("cm");
        assert_eq!(cm, vec!["cmake"]);

        assert_eq!(cache.get_description("cmake"), Some("cross-platform make"));
        assert_eq!(cache.get_description("debhelper"), None);
    }
}
