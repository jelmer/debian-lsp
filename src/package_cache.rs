use std::collections::HashMap;
use std::sync::Arc;
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
    /// Number of packages in the cache.
    fn package_count(&self) -> usize;

    /// Get packages whose names start with a given prefix, sorted.
    fn get_packages_with_prefix(&self, prefix: &str) -> Vec<String>;

    /// Get the cached short description for a package.
    fn get_description(&self, package: &str) -> Option<&str>;

    /// Get cached versions for a package.
    fn get_versions(&self, package: &str) -> Option<&[VersionInfo]>;

    /// Load and cache the short description for a package.
    async fn load_description(&mut self, package: &str) -> Option<String>;

    /// Load and cache versions for a package.
    async fn load_versions(&mut self, package: &str) -> Option<&[VersionInfo]>;

    /// Refresh the package list from the system.
    async fn refresh_packages(&mut self);
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
}

impl AptPackageCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
            descriptions: HashMap::new(),
            versions: HashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl PackageCache for AptPackageCache {
    fn package_count(&self) -> usize {
        self.packages.len()
    }

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

    fn get_versions(&self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    async fn load_description(&mut self, package: &str) -> Option<String> {
        if let Some(desc) = self.descriptions.get(package) {
            return Some(desc.clone());
        }

        match Command::new("apt-cache")
            .arg("show")
            .arg("--no-all-versions")
            .arg(package)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    if let Some(desc) = line.strip_prefix("Description: ") {
                        let description = desc.trim().to_string();
                        self.descriptions
                            .insert(package.to_string(), description.clone());
                        return Some(description);
                    }
                }
                None
            }
            _ => None,
        }
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

    async fn refresh_packages(&mut self) {
        match Command::new("apt-cache").arg("pkgnames").output().await {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                self.packages = text
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                self.packages.sort_unstable();
            }
            _ => {
                // apt-cache not available or failed; keep existing cache
            }
        }
    }
}

/// Create a new shared cache backed by apt-cache.
pub fn new_shared_cache() -> SharedPackageCache {
    Arc::new(RwLock::new(AptPackageCache::new()))
}

/// Simple in-memory package cache for tests.
#[derive(Default)]
pub struct TestPackageCache {
    /// Packages and their optional descriptions.
    pub packages: Vec<(String, Option<String>)>,
    /// Cached versions.
    pub versions: HashMap<String, Vec<VersionInfo>>,
}

#[async_trait::async_trait]
impl PackageCache for TestPackageCache {
    fn package_count(&self) -> usize {
        self.packages.len()
    }

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

    fn get_versions(&self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    async fn load_description(&mut self, _package: &str) -> Option<String> {
        self.get_description(_package).map(|s| s.to_string())
    }

    async fn load_versions(&mut self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    async fn refresh_packages(&mut self) {
        // No-op for test cache
    }
}

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
        assert_eq!(cache.package_count(), 0);
        assert!(cache.get_packages_with_prefix("foo").is_empty());
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
