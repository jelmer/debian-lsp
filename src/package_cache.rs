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

    /// Load and cache versions for multiple packages in a single batch call.
    async fn load_versions_batch(&mut self, packages: &[String]);

    /// Load and cache providers for multiple packages in a single batch call.
    async fn load_providers_batch(&mut self, packages: &[String]);

    /// Get already-cached versions for a package, without triggering a lookup.
    fn get_cached_versions(&self, package: &str) -> Option<&[VersionInfo]>;

    /// Get already-cached providers for a package, without triggering a lookup.
    fn get_cached_providers(&self, package: &str) -> Option<&[String]>;

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

/// Extract the suite name from an apt source line like
/// `500 http://deb.debian.org/debian unstable/main amd64 Packages`.
/// Returns the suite component (e.g. "unstable", "bullseye").
fn extract_suite_from_apt_line(line: &str) -> Option<&str> {
    // Format: "PRIORITY URL SUITE/COMPONENT ARCH TYPE"
    // or:     "PRIORITY /var/lib/dpkg/status"
    let trimmed = line.trim();
    let mut parts = trimmed.split_whitespace();
    let _priority = parts.next()?;
    let url_or_path = parts.next()?;
    if url_or_path.starts_with('/') {
        return None; // local dpkg status, not an archive
    }
    let suite_component = parts.next()?;
    suite_component.split('/').next()
}

/// Parse a single line of `apt-cache policy` output, updating the
/// current version list.  Call this for every line after the package
/// header (the `name:` line).
///
/// Version lines look like `" *** 13.20 500"` or `"     13.11.6 500"` and
/// are indented with up to 5 leading spaces. Suite lines like
/// `"        500 http://... suite/component ..."` have 8+ leading spaces.
fn parse_policy_line(line: &str, versions: &mut Vec<VersionInfo>) {
    let trimmed = line.trim();

    // Count leading spaces to distinguish version lines (<=5 spaces) from
    // suite/source lines (8+ spaces).
    let leading_spaces = line.len() - line.trim_start().len();

    if trimmed.starts_with("***")
        || (leading_spaces < 8 && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()))
    {
        // Version line: "VERSION PRIORITY" or "*** VERSION PRIORITY"
        let version_str = if let Some(rest) = trimmed.strip_prefix("*** ") {
            rest.split_whitespace().next()
        } else {
            trimmed.split_whitespace().next()
        };
        if let Some(v) = version_str {
            versions.push(VersionInfo {
                version: v.to_string(),
                suites: Vec::new(),
            });
        }
    } else if let Some(last) = versions.last_mut() {
        // Suite line under a version
        if let Some(suite) = extract_suite_from_apt_line(trimmed) {
            let suite = suite.to_string();
            if !last.suites.contains(&suite) {
                last.suites.push(suite);
            }
        }
    }
}

/// Parse the full output of `apt-cache policy <single-package>` into
/// a list of `VersionInfo` entries (newest first, following apt order).
fn parse_policy_versions(text: &str) -> Vec<VersionInfo> {
    let mut versions = Vec::new();
    let mut in_version_table = false;

    for line in text.lines() {
        if line.starts_with("  Version table:") {
            in_version_table = true;
            continue;
        }
        if in_version_table {
            parse_policy_line(line, &mut versions);
        }
    }

    versions
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
            .arg("policy")
            .arg(package)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let versions = parse_policy_versions(&text);
                self.versions.insert(package.to_string(), versions);
                self.versions.get(package).map(|v| v.as_slice())
            }
            _ => None,
        }
    }

    async fn load_versions_batch(&mut self, packages: &[String]) {
        let uncached: Vec<&String> = packages
            .iter()
            .filter(|p| !self.versions.contains_key(p.as_str()))
            .collect();
        if uncached.is_empty() {
            return;
        }

        let Ok(output) = Command::new("apt-cache")
            .arg("policy")
            .args(&uncached)
            .output()
            .await
        else {
            return;
        };
        if !output.status.success() {
            return;
        }

        // Initialize empty entries for all requested packages so we don't
        // re-query them on the next call.
        for pkg in &uncached {
            self.versions.entry(pkg.to_string()).or_default();
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_package: Option<String> = None;
        let mut current_versions: Vec<VersionInfo> = Vec::new();
        let mut in_version_table = false;

        for line in text.lines() {
            if !line.starts_with(' ') && line.ends_with(':') {
                // New package header — save previous package's versions
                if let Some(pkg) = current_package.take() {
                    if !current_versions.is_empty() {
                        self.versions
                            .insert(pkg, std::mem::take(&mut current_versions));
                    }
                }
                current_package = Some(line.trim_end_matches(':').to_string());
                current_versions.clear();
                in_version_table = false;
            } else if line.starts_with("  Version table:") {
                in_version_table = true;
            } else if in_version_table {
                parse_policy_line(line, &mut current_versions);
            }
        }
        // Save last package
        if let Some(pkg) = current_package {
            if !current_versions.is_empty() {
                self.versions.insert(pkg, current_versions);
            }
        }
    }

    async fn load_providers_batch(&mut self, packages: &[String]) {
        let uncached: Vec<&String> = packages
            .iter()
            .filter(|p| !self.providers.contains_key(p.as_str()))
            .collect();
        if uncached.is_empty() {
            return;
        }

        let Ok(output) = Command::new("apt-cache")
            .arg("showpkg")
            .args(&uncached)
            .output()
            .await
        else {
            return;
        };
        if !output.status.success() {
            return;
        }

        // Initialize empty entries so we don't re-query.
        for pkg in &uncached {
            self.providers.entry(pkg.to_string()).or_default();
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_package: Option<String> = None;
        let mut in_reverse_provides = false;

        for line in text.lines() {
            if let Some(name) = line.strip_prefix("Package: ") {
                current_package = Some(name.to_string());
                in_reverse_provides = false;
            } else if line.starts_with("Reverse Provides:") {
                in_reverse_provides = true;
            } else if line.starts_with("Versions:")
                || line.starts_with("Reverse Depends:")
                || line.starts_with("Dependencies:")
                || line.starts_with("Provides:")
            {
                in_reverse_provides = false;
            } else if in_reverse_provides {
                if let (Some(pkg), Some(name)) = (&current_package, line.split_whitespace().next())
                {
                    let providers = self.providers.entry(pkg.clone()).or_default();
                    let name = name.to_string();
                    if !providers.contains(&name) {
                        providers.push(name);
                    }
                }
            }
        }

        // Sort providers for consistent display
        for providers in self.providers.values_mut() {
            providers.sort();
        }
    }

    fn get_cached_versions(&self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    fn get_cached_providers(&self, package: &str) -> Option<&[String]> {
        self.providers.get(package).map(|v| v.as_slice())
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

    async fn load_versions_batch(&mut self, _packages: &[String]) {
        // Test cache is pre-populated; nothing to load.
    }

    async fn load_providers_batch(&mut self, _packages: &[String]) {
        // Test cache is pre-populated; nothing to load.
    }

    fn get_cached_versions(&self, package: &str) -> Option<&[VersionInfo]> {
        self.versions.get(package).map(|v| v.as_slice())
    }

    fn get_cached_providers(&self, package: &str) -> Option<&[String]> {
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

    #[test]
    fn test_extract_suite_from_apt_line_normal() {
        let line = "   500 http://deb.debian.org/debian unstable/main amd64 Packages";
        assert_eq!(extract_suite_from_apt_line(line), Some("unstable"));
    }

    #[test]
    fn test_extract_suite_from_apt_line_dpkg_status() {
        let line = "   100 /var/lib/dpkg/status";
        assert_eq!(extract_suite_from_apt_line(line), None);
    }

    #[test]
    fn test_extract_suite_from_apt_line_malformed() {
        assert_eq!(extract_suite_from_apt_line(""), None);
        assert_eq!(extract_suite_from_apt_line("   "), None);
        assert_eq!(extract_suite_from_apt_line("500"), None);
    }

    #[test]
    fn test_parse_policy_line_version() {
        let mut versions = Vec::new();
        parse_policy_line("     2.40-4 500", &mut versions);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "2.40-4");
        assert!(versions[0].suites.is_empty());
    }

    #[test]
    fn test_parse_policy_line_installed_version() {
        let mut versions = Vec::new();
        parse_policy_line(" *** 2.40-4 500", &mut versions);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "2.40-4");
        assert!(versions[0].suites.is_empty());
    }

    #[test]
    fn test_parse_policy_line_suite() {
        let mut versions = vec![VersionInfo {
            version: "2.40-4".to_string(),
            suites: Vec::new(),
        }];
        parse_policy_line(
            "        500 http://deb.debian.org/debian unstable/main amd64 Packages",
            &mut versions,
        );
        assert_eq!(versions[0].suites, vec!["unstable"]);
    }

    #[test]
    fn test_parse_policy_versions_realistic() {
        let output = "\
debhelper:
  Installed: 13.20
  Candidate: 13.20
  Version table:
 *** 13.20 500
        500 http://deb.debian.org/debian unstable/main amd64 Packages
        100 /var/lib/dpkg/status
     13.11.6 500
        500 http://deb.debian.org/debian bookworm/main amd64 Packages
";
        let versions = parse_policy_versions(output);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, "13.20");
        assert_eq!(versions[0].suites, vec!["unstable"]);
        assert_eq!(versions[1].version, "13.11.6");
        assert_eq!(versions[1].suites, vec!["bookworm"]);
    }
}
