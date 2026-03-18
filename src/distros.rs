//! Cached Debian distribution information from distro-info-data.
//!
//! Loads `DebianDistroInfo` once and provides distribution names,
//! suite-to-codename mappings (date-aware), and per-release metadata.

use std::sync::OnceLock;

use chrono::NaiveDate;
use distro_info::{DebianDistroInfo, DistroInfo};

/// Cached data derived from distro-info-data.
struct DistroData {
    /// All known distribution names (aliases + codenames), for completions.
    distributions: Vec<String>,

    /// The parsed distro-info data, if available.
    debian_info: Option<DebianDistroInfo>,
}

fn load_distro_data() -> DistroData {
    let mut distributions = vec![
        "unstable".to_string(),
        "stable".to_string(),
        "testing".to_string(),
        "oldstable".to_string(),
        "experimental".to_string(),
        "sid".to_string(),
        "UNRELEASED".to_string(),
    ];

    let Ok(debian_info) = DebianDistroInfo::new() else {
        return DistroData {
            distributions,
            debian_info: None,
        };
    };

    for release in debian_info.iter() {
        let series = release.series().to_string();
        if !distributions.contains(&series) {
            distributions.push(series);
        }
    }

    DistroData {
        distributions,
        debian_info: Some(debian_info),
    }
}

static DISTRO_DATA: OnceLock<DistroData> = OnceLock::new();

fn cached() -> &'static DistroData {
    DISTRO_DATA.get_or_init(load_distro_data)
}

/// Resolved suite codenames for a given date.
struct SuiteResolution {
    testing: Option<String>,
    stable: Option<String>,
    oldstable: Option<String>,
}

/// Resolve testing/stable/oldstable codenames for the given date.
fn resolve_suites(debian_info: &DebianDistroInfo, date: NaiveDate) -> SuiteResolution {
    let supported = debian_info.supported(date);
    let mut released_supported: Vec<_> = supported
        .iter()
        .filter(|r| r.version().is_some() && r.release().is_some_and(|released| released <= date))
        .collect();
    released_supported.sort_by_key(|r| r.release());

    let stable = released_supported.last().map(|r| r.series().to_string());

    let oldstable = if released_supported.len() >= 2 {
        Some(
            released_supported[released_supported.len() - 2]
                .series()
                .to_string(),
        )
    } else {
        None
    };

    // testing = the next release that has a version number but hasn't been released
    // at this date (either no release date or release date is in the future)
    let testing = debian_info
        .iter()
        .find(|r| r.version().is_some() && r.release().is_none_or(|released| released > date))
        .map(|r| r.series().to_string());

    SuiteResolution {
        testing,
        stable,
        oldstable,
    }
}

/// Whether distro-info-data is available on this system.
#[cfg(test)]
pub fn has_distro_info() -> bool {
    cached().debian_info.is_some()
}

/// Get all known Debian distribution names (aliases + codenames), for completions.
pub fn get_all_distributions() -> &'static [String] {
    &cached().distributions
}

/// Map a distribution alias to its codename or vice versa, using today's date.
///
/// Convenience wrapper around [`get_distribution_mapping_at`].
pub fn get_distribution_mapping(distribution: &str) -> Option<String> {
    let today = chrono::Local::now().date_naive();
    get_distribution_mapping_at(distribution, today)
}

/// Map a distribution alias to its codename or vice versa at the given date.
///
/// Returns `None` if there is no mapping (e.g. the distribution is
/// unambiguous, or distro-info data is unavailable).
///
/// Examples (with today's date):
/// - `"unstable"` → `Some("sid")`
/// - `"sid"` → `Some("unstable")`
/// - `"testing"` → `Some("forky")` (current testing codename)
/// - `"trixie"` → `Some("stable")` (current stable codename)
/// - `"experimental"` → `None`
pub fn get_distribution_mapping_at(distribution: &str, date: NaiveDate) -> Option<String> {
    let data = cached();

    match distribution {
        "unstable" => Some("sid".to_string()),
        "sid" => Some("unstable".to_string()),
        "experimental" => None,
        "testing" | "stable" | "oldstable" => {
            let Some(debian_info) = &data.debian_info else {
                return None;
            };
            let suites = resolve_suites(debian_info, date);
            match distribution {
                "testing" => suites.testing,
                "stable" => suites.stable,
                "oldstable" => suites.oldstable,
                _ => unreachable!(),
            }
        }
        codename => {
            let Some(debian_info) = &data.debian_info else {
                return None;
            };
            let suites = resolve_suites(debian_info, date);
            if suites.testing.as_deref() == Some(codename) {
                Some("testing".to_string())
            } else if suites.stable.as_deref() == Some(codename) {
                Some("stable".to_string())
            } else if suites.oldstable.as_deref() == Some(codename) {
                Some("oldstable".to_string())
            } else {
                None
            }
        }
    }
}

/// Get a short detail string for a distribution name, suitable for the
/// `detail` field of a completion item.
///
/// For aliases like "testing", returns the codename (e.g. "forky").
/// For codenames like "trixie", returns the alias and version (e.g. "stable, Debian 13").
pub fn get_distribution_detail(distribution: &str) -> Option<String> {
    let data = cached();
    let today = chrono::Local::now().date_naive();

    // Resolve the codename: for aliases use the mapping, for codenames use as-is.
    let debian_info = data.debian_info.as_ref();
    let suites = debian_info.map(|di| resolve_suites(di, today));

    let codename = match distribution {
        "UNRELEASED" => return None,
        "unstable" | "sid" => Some("sid"),
        "testing" => suites.as_ref().and_then(|s| s.testing.as_deref()),
        "stable" => suites.as_ref().and_then(|s| s.stable.as_deref()),
        "oldstable" => suites.as_ref().and_then(|s| s.oldstable.as_deref()),
        other => Some(other),
    };

    let mut parts = Vec::new();

    // Show the suite mapping if there is one
    if let Some(mapped) = get_distribution_mapping(distribution) {
        parts.push(mapped);
    }

    // Show version and dates from release info
    if let Some(release) =
        codename.and_then(|c| debian_info.and_then(|di| di.iter().find(|r| r.series() == c)))
    {
        if let Some(version) = release.version() {
            parts.push(format!("Debian {}", version));
        }
        if let Some(released) = release.release() {
            parts.push(format!("released {}", released));
        }
        if let Some(eol) = release.eol() {
            if *eol < today {
                parts.push(format!("end of life since {}", eol));
            } else {
                parts.push(format!("EOL {}", eol));
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unstable_maps_to_sid() {
        assert_eq!(
            get_distribution_mapping("unstable"),
            Some("sid".to_string())
        );
    }

    #[test]
    fn test_sid_maps_to_unstable() {
        assert_eq!(
            get_distribution_mapping("sid"),
            Some("unstable".to_string())
        );
    }

    #[test]
    fn test_experimental_has_no_mapping() {
        assert_eq!(get_distribution_mapping("experimental"), None);
    }

    #[test]
    fn test_testing_maps_to_codename() {
        let result = get_distribution_mapping("testing");
        if let Some(ref codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
        }
    }

    #[test]
    fn test_stable_maps_to_codename() {
        let result = get_distribution_mapping("stable");
        if let Some(ref codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
            assert_ne!(codename, "unstable");
        }
    }

    #[test]
    fn test_suite_codenames_are_distinct() {
        let today = chrono::Local::now().date_naive();
        let Some(debian_info) = &cached().debian_info else {
            return;
        };
        let suites = resolve_suites(debian_info, today);
        let mut seen = std::collections::HashSet::new();
        for name in [&suites.testing, &suites.stable, &suites.oldstable]
            .into_iter()
            .flatten()
        {
            assert!(seen.insert(name.as_str()), "Duplicate codename: {}", name);
        }
    }

    #[test]
    fn test_get_all_distributions_includes_aliases() {
        let dists = get_all_distributions();
        assert!(dists.contains(&"unstable".to_string()));
        assert!(dists.contains(&"stable".to_string()));
        assert!(dists.contains(&"testing".to_string()));
        assert!(dists.contains(&"UNRELEASED".to_string()));
        assert!(dists.contains(&"sid".to_string()));
    }

    #[test]
    fn test_get_all_distributions_includes_codenames() {
        if cached().debian_info.is_none() {
            return; // distro-info-data not available (e.g. Windows)
        }
        let dists = get_all_distributions();
        // Should have more than just the aliases (if distro-info is available)
        assert!(dists.len() > 7);
    }

    #[test]
    fn test_codename_reverse_mapping() {
        if let Some(codename) = get_distribution_mapping("testing") {
            assert_eq!(
                get_distribution_mapping(&codename),
                Some("testing".to_string())
            );
        }
        if let Some(codename) = get_distribution_mapping("stable") {
            assert_eq!(
                get_distribution_mapping(&codename),
                Some("stable".to_string())
            );
        }
        if let Some(codename) = get_distribution_mapping("oldstable") {
            assert_eq!(
                get_distribution_mapping(&codename),
                Some("oldstable".to_string())
            );
        }
    }

    #[test]
    fn test_stable_in_2020_was_buster() {
        if cached().debian_info.is_none() {
            return; // distro-info-data not available (e.g. Windows)
        }
        let date = NaiveDate::from_ymd_opt(2020, 6, 1).unwrap();
        assert_eq!(
            get_distribution_mapping_at("stable", date),
            Some("buster".to_string())
        );
    }

    #[test]
    fn test_stable_in_2024_was_bookworm() {
        if cached().debian_info.is_none() {
            return; // distro-info-data not available (e.g. Windows)
        }
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        assert_eq!(
            get_distribution_mapping_at("stable", date),
            Some("bookworm".to_string())
        );
    }

    #[test]
    fn test_oldstable_in_2024_was_bullseye() {
        if cached().debian_info.is_none() {
            return; // distro-info-data not available (e.g. Windows)
        }
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        assert_eq!(
            get_distribution_mapping_at("oldstable", date),
            Some("bullseye".to_string())
        );
    }

    #[test]
    fn test_buster_was_stable_in_2020() {
        if cached().debian_info.is_none() {
            return; // distro-info-data not available (e.g. Windows)
        }
        let date = NaiveDate::from_ymd_opt(2020, 6, 1).unwrap();
        assert_eq!(
            get_distribution_mapping_at("buster", date),
            Some("stable".to_string())
        );
    }

    #[test]
    fn test_unstable_detail_contains_sid() {
        let detail = get_distribution_detail("unstable").unwrap();
        assert!(detail.contains("sid"), "Expected 'sid' in: {}", detail);
    }

    #[test]
    fn test_sid_detail_contains_unstable() {
        let detail = get_distribution_detail("sid").unwrap();
        assert!(
            detail.contains("unstable"),
            "Expected 'unstable' in: {}",
            detail
        );
    }

    #[test]
    fn test_stable_detail_contains_version() {
        if let Some(detail) = get_distribution_detail("stable") {
            assert!(
                detail.contains("Debian"),
                "Expected 'Debian' in: {}",
                detail
            );
        }
    }

    #[test]
    fn test_codename_detail_contains_alias() {
        if let Some(codename) = get_distribution_mapping("stable") {
            let detail = get_distribution_detail(&codename).unwrap();
            assert!(
                detail.contains("stable"),
                "Expected 'stable' in: {}",
                detail
            );
        }
    }

    #[test]
    fn test_unreleased_has_no_detail() {
        assert_eq!(get_distribution_detail("UNRELEASED"), None);
    }

    #[test]
    fn test_experimental_has_no_detail() {
        // experimental has no version or dates in distro-info
        assert_eq!(get_distribution_detail("experimental"), None);
    }
}
