//! Code lenses for debian/control files.
//!
//! - Standards-Version: shows "latest: 4.7.0" when outdated, with an action to update
//! - debhelper-compat: shows compat level info from dh_assistant
//! - Vcs-Git: shows packaged version from UDD vcswatch

use debian_control::relations::VersionConstraint;
use tower_lsp_server::ls_types::{CodeLens, Command, Range};

use crate::position::text_range_to_lsp_range;

/// Command name for opening a URL via `window/showDocument`.
pub const OPEN_URL_COMMAND: &str = "debian-lsp.openUrl";

/// Create a code lens that opens the given URL when clicked.
fn make_link_lens(range: Range, title: String, url: String) -> CodeLens {
    CodeLens {
        range,
        command: Some(Command {
            title,
            command: OPEN_URL_COMMAND.to_string(),
            arguments: Some(vec![serde_json::Value::String(url)]),
        }),
        data: None,
    }
}

/// Create a code lens that is informational only (not clickable).
fn make_info_lens(range: Range, title: String) -> CodeLens {
    CodeLens {
        range,
        command: Some(Command {
            title,
            command: "debian-lsp.noop".to_string(),
            arguments: None,
        }),
        data: None,
    }
}

/// Context for generating code lenses.
pub struct LensContext<'a> {
    /// Cache for package version lookups.
    pub package_cache: &'a crate::package_cache::SharedPackageCache,
    /// Cache for VCS watch lookups from UDD.
    pub vcswatch_cache: &'a crate::vcswatch::SharedVcsWatchCache,
    /// Cache for bug lookups from UDD.
    pub bug_cache: &'a crate::bugs::SharedBugCache,
    /// Cache for popcon lookups from UDD.
    pub popcon_cache: &'a crate::popcon::SharedPopconCache,
    /// Cache for reverse dependency lookups from UDD.
    pub rdeps_cache: &'a crate::rdeps::SharedRdepsCache,
}

/// Info about a Standards-Version field found in the control file.
struct StandardsVersionField {
    /// The value of the Standards-Version field (trimmed).
    value: String,
    /// The LSP range of the entire field entry.
    range: Range,
}

/// Info about a debhelper-compat relation found in the control file.
struct DebhelperCompatField {
    /// The LSP range of the relation.
    range: Range,
}

/// Info about a binary package paragraph found in the control file.
struct BinaryPackageField {
    /// The binary package name.
    name: String,
    /// The LSP range of the Package field entry.
    range: Range,
}

/// Info about the source package paragraph.
struct SourcePackageField {
    /// The source package name.
    name: String,
    /// The LSP range of the Source field entry.
    range: Range,
}

/// Extract the Standards-Version (first 3 components) from a debian-policy
/// package version string like "4.7.3.0" → "4.7.3".
fn policy_version_to_standards_version(policy_version: &str) -> Option<&str> {
    let mut dots = 0;
    for (i, c) in policy_version.char_indices() {
        if c == '.' {
            dots += 1;
            if dots == 3 {
                return Some(&policy_version[..i]);
            }
        }
    }
    if dots >= 2 {
        Some(policy_version)
    } else {
        None
    }
}

/// Compat level info from dh_assistant.
#[derive(serde::Deserialize)]
struct CompatLevels {
    /// The highest compat level available.
    #[serde(rename = "MAX_COMPAT_LEVEL")]
    max: u32,
    /// The highest compat level considered stable.
    #[serde(rename = "HIGHEST_STABLE_COMPAT_LEVEL")]
    highest_stable: u32,
}

/// Query dh_assistant for supported compat levels.
async fn get_compat_levels() -> Option<CompatLevels> {
    let output = tokio::process::Command::new("dh_assistant")
        .arg("supported-compat-levels")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Compare two dotted version strings and return true if `current` is older
/// than `latest`.
fn is_outdated(current: &str, latest: &str) -> bool {
    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();
    let latest_parts: Vec<u32> = latest.split('.').filter_map(|s| s.parse().ok()).collect();

    for (c, l) in current_parts.iter().zip(latest_parts.iter()) {
        match c.cmp(l) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => continue,
        }
    }

    current_parts.len() < latest_parts.len()
}

/// All extracted lens data from a parsed control file.
struct LensData {
    standards_versions: Vec<StandardsVersionField>,
    debhelper_compats: Vec<DebhelperCompatField>,
    source_package: Option<SourcePackageField>,
    binary_packages: Vec<BinaryPackageField>,
}

/// Extract Standards-Version, debhelper-compat, and package fields from a parsed control file.
fn extract_lens_data(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
) -> LensData {
    let control = parsed.tree();
    let mut standards_versions = Vec::new();
    let mut debhelper_compats = Vec::new();
    let mut source_package = None;
    let mut binary_packages = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let Some(field_name) = entry.key() else {
                continue;
            };

            if field_name.eq_ignore_ascii_case("Source") {
                let value = entry.value().trim().to_string();
                if !value.is_empty() {
                    let range = text_range_to_lsp_range(source_text, entry.text_range());
                    source_package = Some(SourcePackageField { name: value, range });
                }
                continue;
            }

            if field_name.eq_ignore_ascii_case("Package") {
                let value = entry.value().trim().to_string();
                if !value.is_empty() {
                    let range = text_range_to_lsp_range(source_text, entry.text_range());
                    binary_packages.push(BinaryPackageField { name: value, range });
                }
                continue;
            }

            if field_name.eq_ignore_ascii_case("Standards-Version") {
                let value = entry.value().trim().to_string();
                if !value.is_empty() {
                    let range = text_range_to_lsp_range(source_text, entry.text_range());
                    standards_versions.push(StandardsVersionField { value, range });
                }
                continue;
            }

            if !super::relation_completion::is_relationship_field(&field_name) {
                continue;
            }

            let value = entry.value();
            let (parsed_rels, _errors) =
                debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
            let line_ranges = entry.value_line_ranges();

            for rel_entry in parsed_rels.entries() {
                for relation in rel_entry.relations() {
                    let Some(name) = relation.try_name() else {
                        continue;
                    };
                    if name != "debhelper-compat" {
                        continue;
                    }
                    if matches!(relation.version(), Some((VersionConstraint::Equal, _))) {
                        let rel_range = relation.syntax().text_range();
                        let rel_end: usize = rel_range.end().into();
                        if let Some(abs_end) = super::inlay_hints::joined_offset_to_source_offset(
                            &line_ranges,
                            rel_end,
                        ) {
                            let rel_start: usize = rel_range.start().into();
                            if let Some(abs_start) =
                                super::inlay_hints::joined_offset_to_source_offset(
                                    &line_ranges,
                                    rel_start,
                                )
                            {
                                let range = text_range_to_lsp_range(
                                    source_text,
                                    text_size::TextRange::new(abs_start, abs_end),
                                );
                                debhelper_compats.push(DebhelperCompatField { range });
                            }
                        }
                    }
                }
            }
        }
    }

    LensData {
        standards_versions,
        debhelper_compats,
        source_package,
        binary_packages,
    }
}

/// Find the Vcs-Git field in a parsed control file and return its URL and range.
fn find_vcs_git_field(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
) -> Option<(String, Range)> {
    let control = parsed.clone().to_result().ok()?;
    let source = control.source()?;
    let vcs_git_value = source.vcs_git()?;
    let parsed_vcs = vcs_git_value
        .parse::<debian_control::vcs::ParsedVcs>()
        .ok()?;

    let entry_range = source
        .as_deb822()
        .entries()
        .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("Vcs-Git")))?
        .text_range();

    let range = text_range_to_lsp_range(source_text, entry_range);
    Some((parsed_vcs.repo_url, range))
}

/// Format a count for display, using k/M suffixes for large numbers.
fn format_count(count: u32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Items that need background fetching before lenses can be generated.
#[derive(Default)]
pub struct UncachedLensData {
    /// Source package name needing bug count lookup.
    pub source_package: Option<String>,
    /// Binary package names needing popcon/rdeps lookups.
    pub binary_packages: Vec<String>,
    /// Whether the Standards-Version policy package needs fetching.
    pub needs_policy_version: bool,
    /// Vcs-Git URL needing vcswatch lookup.
    pub vcs_git_url: Option<String>,
}

impl UncachedLensData {
    /// Returns `true` if there is nothing to fetch.
    pub fn is_empty(&self) -> bool {
        self.source_package.is_none()
            && self.binary_packages.is_empty()
            && !self.needs_policy_version
            && self.vcs_git_url.is_none()
    }
}

/// Generate code lenses for a control file, using only cached data.
///
/// Returns the lenses that can be produced immediately plus a description of
/// what data is still missing. The caller should fetch the missing data in
/// the background and request a code lens refresh when done.
///
/// The debhelper-compat lens requires running `dh_assistant`, which is fast
/// and local, so it is always awaited inline.
pub async fn generate_code_lenses(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    ctx: &LensContext<'_>,
) -> (Vec<CodeLens>, UncachedLensData) {
    let data = extract_lens_data(parsed, source_text);
    let mut lenses = Vec::new();
    let mut uncached = UncachedLensData::default();

    // Standards-Version lens (cache-only read for policy version)
    if !data.standards_versions.is_empty() {
        let cache = ctx.package_cache.read().await;
        let latest_standards = cache.get_cached_versions("debian-policy").and_then(|vs| {
            vs.first()
                .and_then(|v| policy_version_to_standards_version(&v.version))
                .map(|s| s.to_string())
        });
        drop(cache);

        if let Some(latest) = latest_standards {
            for sv in &data.standards_versions {
                if sv.value == latest || !is_outdated(&sv.value, &latest) {
                    continue;
                }
                lenses.push(make_link_lens(
                    sv.range,
                    format!("latest: {}", latest),
                    format!(
                        "https://www.debian.org/doc/debian-policy/upgrading-checklist.html#version-{}",
                        latest.replace('.', "-")
                    ),
                ));
            }
        } else {
            uncached.needs_policy_version = true;
        }
    }

    // debhelper-compat lens (local dh_assistant call, always awaited)
    if !data.debhelper_compats.is_empty() {
        if let Some(levels) = get_compat_levels().await {
            for dh in &data.debhelper_compats {
                let title = if levels.max == levels.highest_stable {
                    format!("stable: {}", levels.highest_stable)
                } else {
                    format!("stable: {}, max: {}", levels.highest_stable, levels.max)
                };
                lenses.push(make_info_lens(dh.range, title));
            }
        }
    }

    // Vcs-Git lens (cache-only)
    if let Some((url, range)) = find_vcs_git_field(parsed, source_text) {
        let cache = ctx.vcswatch_cache.read().await;
        let version = cache
            .get_cached_version_for_url(&url)
            .map(|s| s.to_string());
        let is_cached = cache.is_cached(&url);
        drop(cache);

        if let Some(version) = version {
            lenses.push(make_link_lens(
                range,
                format!("git: {}", version),
                url.clone(),
            ));
        } else if !is_cached {
            uncached.vcs_git_url = Some(url);
        }
    }

    // Source package bug count lens (cache-only)
    if let Some(source) = &data.source_package {
        let cache = ctx.bug_cache.read().await;
        let bug_count = cache.get_cached_open_bug_count(&source.name);
        drop(cache);

        match bug_count {
            Some(count) if count > 0 => {
                lenses.push(make_link_lens(
                    source.range,
                    format!("{} open {}", count, if count == 1 { "bug" } else { "bugs" }),
                    format!("https://bugs.debian.org/src:{}", source.name),
                ));
            }
            None => {
                uncached.source_package = Some(source.name.clone());
            }
            _ => {}
        }
    }

    // Binary package lenses: popcon + rdeps (cache-only, separate clickable lenses)
    for pkg in &data.binary_packages {
        let mut needs_fetch = false;

        {
            let cache = ctx.popcon_cache.read().await;
            if let Some(count) = cache.get_cached_inst_count(&pkg.name) {
                lenses.push(make_link_lens(
                    pkg.range,
                    format!("popcon: {} installs", format_count(count)),
                    format!(
                        "https://qa.debian.org/popcon-graph.php?packages={}",
                        pkg.name
                    ),
                ));
            } else if !cache.is_cached(&pkg.name) {
                needs_fetch = true;
            }
        }

        {
            let cache = ctx.rdeps_cache.read().await;
            if let Some(count) = cache.get_cached_rdeps_count(&pkg.name) {
                if count > 0 {
                    lenses.push(make_link_lens(
                        pkg.range,
                        format!("{} reverse deps", format_count(count)),
                        format!("https://tracker.debian.org/pkg/{}", pkg.name),
                    ));
                }
            } else if !cache.is_cached(&pkg.name) {
                needs_fetch = true;
            }
        }

        if needs_fetch {
            uncached.binary_packages.push(pkg.name.clone());
        }
    }

    (lenses, uncached)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shared_vcswatch_cache() -> crate::vcswatch::SharedVcsWatchCache {
        use crate::vcswatch::VcsWatchCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        Arc::new(RwLock::new(VcsWatchCache::new(crate::udd::shared_pool())))
    }

    fn make_shared_bug_cache() -> crate::bugs::SharedBugCache {
        crate::bugs::new_shared_bug_cache(crate::udd::shared_pool())
    }

    fn make_shared_popcon_cache() -> crate::popcon::SharedPopconCache {
        crate::popcon::new_shared_popcon_cache(crate::udd::shared_pool())
    }

    fn make_shared_rdeps_cache() -> crate::rdeps::SharedRdepsCache {
        crate::rdeps::new_shared_rdeps_cache(crate::udd::shared_pool())
    }

    #[tokio::test]
    async fn test_code_lens_outdated_standards_version() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let content =
            "Source: test-package\nStandards-Version: 4.6.2\nMaintainer: Test <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "latest: 4.7.3");
    }

    #[tokio::test]
    async fn test_no_code_lens_when_current() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let content =
            "Source: test-package\nStandards-Version: 4.7.3\nMaintainer: Test <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 0);
    }

    #[tokio::test]
    async fn test_code_lens_debhelper_compat() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Skip if dh_assistant is not installed
        if tokio::process::Command::new("dh_assistant")
            .arg("supported-compat-levels")
            .output()
            .await
            .is_err()
        {
            return;
        }

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let content = "\
Source: test-package
Build-Depends: debhelper-compat (= 13), pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        let title = &lenses[0].command.as_ref().unwrap().title;
        assert!(
            title.starts_with("stable: "),
            "expected title starting with 'stable: ', got: {}",
            title
        );
    }

    #[tokio::test]
    async fn test_both_standards_version_and_debhelper_compat_lenses() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Skip if dh_assistant is not installed
        if tokio::process::Command::new("dh_assistant")
            .arg("supported-compat-levels")
            .output()
            .await
            .is_err()
        {
            return;
        }

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let content = "\
Source: test-package
Standards-Version: 4.6.2
Build-Depends: debhelper-compat (= 13), pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 2);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "latest: 4.7.3");
        assert!(lenses[1]
            .command
            .as_ref()
            .unwrap()
            .title
            .starts_with("stable: "));
    }

    #[tokio::test]
    async fn test_code_lens_vcs_git() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let mut vcs_cache = crate::vcswatch::VcsWatchCache::new(crate::udd::shared_pool());
        vcs_cache.insert_cached(
            "https://salsa.debian.org/python-team/packages/dulwich.git",
            "1.1.0-1",
        );
        let vcswatch_cache: crate::vcswatch::SharedVcsWatchCache = Arc::new(RwLock::new(vcs_cache));

        let content = "Source: dulwich\nVcs-Git: https://salsa.debian.org/python-team/packages/dulwich.git\nVcs-Browser: https://salsa.debian.org/python-team/packages/dulwich\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "git: 1.1.0-1");
    }

    #[tokio::test]
    async fn test_code_lens_vcs_git_with_branch_suffix() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let mut vcs_cache = crate::vcswatch::VcsWatchCache::new(crate::udd::shared_pool());
        vcs_cache.insert_cached("https://salsa.debian.org/team/pkg.git", "2.0-1");
        let vcswatch_cache: crate::vcswatch::SharedVcsWatchCache = Arc::new(RwLock::new(vcs_cache));

        let content =
            "Source: pkg\nVcs-Git: https://salsa.debian.org/team/pkg.git -b debian/latest\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "git: 2.0-1");
    }

    #[tokio::test]
    async fn test_no_code_lens_vcs_git_when_not_in_vcswatch() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let content = "Source: unknown\nVcs-Git: https://example.com/unknown.git\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let bug_cache = make_shared_bug_cache();
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();
        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 0);
    }

    #[tokio::test]
    async fn test_code_lens_source_bug_count() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();

        let bug_cache = {
            let mut bc = crate::bugs::BugCache::new(crate::udd::shared_pool());
            bc.insert_cached_open_bugs_for_package(
                "test-package",
                vec![
                    (100001, Some("Bug one")),
                    (100002, Some("Bug two")),
                    (100003, Some("Bug three")),
                ],
            );
            Arc::new(RwLock::new(bc))
        };
        let popcon_cache = make_shared_popcon_cache();
        let rdeps_cache = make_shared_rdeps_cache();

        let content = "Source: test-package\nMaintainer: Test <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(content);

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "3 open bugs");
        assert_eq!(
            lenses[0].command.as_ref().unwrap().command,
            OPEN_URL_COMMAND
        );
    }

    #[tokio::test]
    async fn test_code_lens_binary_package_popcon_and_rdeps() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));
        let vcswatch_cache = make_shared_vcswatch_cache();
        let bug_cache = make_shared_bug_cache();

        let popcon_cache = {
            let mut pc = crate::popcon::PopconCache::new(crate::udd::shared_pool());
            pc.insert_cached("libfoo1", 42000);
            Arc::new(RwLock::new(pc))
        };
        let rdeps_cache = {
            let mut rc = crate::rdeps::RdepsCache::new(crate::udd::shared_pool());
            rc.insert_cached("libfoo1", 150);
            Arc::new(RwLock::new(rc))
        };

        let content = "\
Source: foo
Maintainer: Test <test@example.com>

Package: libfoo1
Architecture: any
Description: Foo library
";
        let parsed = debian_control::lossless::Control::parse(content);

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
            bug_cache: &bug_cache,
            popcon_cache: &popcon_cache,
            rdeps_cache: &rdeps_cache,
        };
        let (lenses, _uncached) = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "popcon: 42.0k installs"
        );
        assert_eq!(
            lenses[0].command.as_ref().unwrap().command,
            OPEN_URL_COMMAND
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().title,
            "150 reverse deps"
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().command,
            OPEN_URL_COMMAND
        );
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1.0k");
        assert_eq!(format_count(42000), "42.0k");
        assert_eq!(format_count(1_500_000), "1.5M");
    }
}
