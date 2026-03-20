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

/// Extract Standards-Version and debhelper-compat fields from a parsed control file.
fn extract_lens_data(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
) -> (Vec<StandardsVersionField>, Vec<DebhelperCompatField>) {
    let control = parsed.tree();
    let mut standards_versions = Vec::new();
    let mut debhelper_compats = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let Some(field_name) = entry.key() else {
                continue;
            };

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

    (standards_versions, debhelper_compats)
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

/// Generate code lenses for a control file.
///
/// Provides lenses for:
/// - Standards-Version: shows latest version when outdated
/// - debhelper-compat (= N): shows current compat level
/// - Vcs-Git: shows packaged version from UDD vcswatch
pub async fn generate_code_lenses(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    ctx: &LensContext<'_>,
) -> Vec<CodeLens> {
    let (standards_versions, debhelper_compats) = extract_lens_data(parsed, source_text);

    let latest_standards = if !standards_versions.is_empty() {
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

    let compat_levels = if !debhelper_compats.is_empty() {
        get_compat_levels().await
    } else {
        None
    };

    let mut lenses = Vec::new();

    if let Some(latest) = &latest_standards {
        for sv in &standards_versions {
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
        for dh in &debhelper_compats {
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
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

        let ctx = LensContext {
            package_cache: &shared_cache,
            vcswatch_cache: &vcswatch_cache,
        };
        let lenses = generate_code_lenses(&parsed, content, &ctx).await;

        assert_eq!(lenses.len(), 0);
    }
}
