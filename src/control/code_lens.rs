//! Code lenses for debian/control files.
//!
//! - Standards-Version: shows "latest: 4.7.0" when outdated, with an action to update
//! - debhelper-compat: shows compat level info from dh_assistant
//! - Vcs-Git: shows packaged version from UDD vcswatch

use debian_control::relations::VersionConstraint;
use tower_lsp_server::ls_types::{CodeLens, Command, Range};

use crate::position::text_range_to_lsp_range;

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

/// Generate code lenses for a control file.
///
/// Provides lenses for:
/// - Standards-Version: shows latest version when outdated
/// - debhelper-compat (= N): shows current compat level
/// - Vcs-Git: shows packaged version from UDD vcswatch
/// - Source package: shows open bug count
/// - Binary packages: shows popcon install count and reverse dependency count
pub async fn generate_code_lenses(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    ctx: &LensContext<'_>,
) -> Vec<CodeLens> {
    let data = extract_lens_data(parsed, source_text);

    let latest_standards = if !data.standards_versions.is_empty() {
        let mut cache = ctx.package_cache.write().await;
        let versions = cache.load_versions("debian-policy").await;
        versions.and_then(|vs| {
            vs.first()
                .and_then(|v| policy_version_to_standards_version(&v.version))
                .map(|s| s.to_string())
        })
    } else {
        None
    };

    let compat_levels = if !data.debhelper_compats.is_empty() {
        get_compat_levels().await
    } else {
        None
    };

    let mut lenses = Vec::new();

    if let Some(latest) = &latest_standards {
        for sv in &data.standards_versions {
            if sv.value == *latest || !is_outdated(&sv.value, latest) {
                continue;
            }
            lenses.push(CodeLens {
                range: sv.range,
                command: Some(Command {
                    title: format!("latest: {}", latest),
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            });
        }
    }

    if let Some(levels) = &compat_levels {
        for dh in &data.debhelper_compats {
            let title = if levels.max == levels.highest_stable {
                format!("stable: {}", levels.highest_stable)
            } else {
                format!("stable: {}, max: {}", levels.highest_stable, levels.max)
            };
            lenses.push(CodeLens {
                range: dh.range,
                command: Some(Command {
                    title,
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            });
        }
    }

    // Vcs-Git lens
    if let Some((url, range)) = find_vcs_git_field(parsed, source_text) {
        let version = {
            let mut cache = ctx.vcswatch_cache.write().await;
            cache.get_version_for_url(&url).await.map(|s| s.to_string())
        };
        if let Some(version) = version {
            lenses.push(CodeLens {
                range,
                command: Some(Command {
                    title: format!("git: {}", version),
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            });
        }
    }

    // Source package bug count lens
    if let Some(source) = &data.source_package {
        let bug_count = {
            let mut cache = ctx.bug_cache.write().await;
            cache.get_open_bug_count(&source.name).await
        };
        if bug_count > 0 {
            lenses.push(CodeLens {
                range: source.range,
                command: Some(Command {
                    title: format!(
                        "{} open {}",
                        bug_count,
                        if bug_count == 1 { "bug" } else { "bugs" }
                    ),
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            });
        }
    }

    // Binary package lenses: popcon + rdeps
    for pkg in &data.binary_packages {
        let mut parts = Vec::new();

        let popcon = {
            let mut cache = ctx.popcon_cache.write().await;
            cache.get_inst_count(&pkg.name).await
        };
        if let Some(count) = popcon {
            parts.push(format!("popcon: {} installs", format_count(count)));
        }

        let rdeps = {
            let mut cache = ctx.rdeps_cache.write().await;
            cache.get_rdeps_count(&pkg.name).await
        };
        if let Some(count) = rdeps {
            if count > 0 {
                parts.push(format!("{} reverse deps", format_count(count)));
            }
        }

        if !parts.is_empty() {
            lenses.push(CodeLens {
                range: pkg.range,
                command: Some(Command {
                    title: parts.join(" | "),
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            });
        }
    }

    lenses
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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "3 open bugs");
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
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "popcon: 42.0k installs | 150 reverse deps"
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
