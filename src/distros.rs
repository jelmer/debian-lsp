//! Cached Debian distribution information from distro-info-data.
//!
//! Loads `DebianDistroInfo` once and caches derived data (distribution names,
//! suite-to-codename mappings) for the lifetime of the process.

use std::sync::OnceLock;

use distro_info::{DebianDistroInfo, DistroInfo};

/// Cached distribution data derived from distro-info-data.
struct DistroData {
    /// All known distribution names (aliases + codenames), for completions.
    distributions: Vec<String>,

    /// Current testing codename (e.g. "forky").
    testing: Option<String>,

    /// Current stable codename (e.g. "trixie").
    stable: Option<String>,

    /// Current oldstable codename (e.g. "bookworm").
    oldstable: Option<String>,
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
            testing: None,
            stable: None,
            oldstable: None,
        };
    };

    // Add all release codenames
    for release in debian_info.iter() {
        let series = release.series();
        if !distributions.contains(&series.to_string()) {
            distributions.push(series.to_string());
        }
    }

    let today = chrono::Local::now().date_naive();

    // Supported releases with actual version numbers and release dates
    // (excludes sid and experimental which have no version).
    let supported = debian_info.supported(today);
    let mut released_supported: Vec<_> = supported
        .iter()
        .filter(|r| r.version().is_some() && r.release().is_some())
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

    // testing = has a version number but no release date yet (not sid/experimental)
    let testing = debian_info
        .iter()
        .find(|r| r.version().is_some() && r.release().is_none())
        .map(|r| r.series().to_string());

    DistroData {
        distributions,
        testing,
        stable,
        oldstable,
    }
}

static DISTRO_DATA: OnceLock<DistroData> = OnceLock::new();

fn cached() -> &'static DistroData {
    DISTRO_DATA.get_or_init(load_distro_data)
}

/// Get all known Debian distribution names (aliases + codenames), for completions.
pub fn get_all_distributions() -> &'static [String] {
    &cached().distributions
}

/// Map a distribution alias to its codename or vice versa.
///
/// Returns `None` if there is no mapping (e.g. the distribution is
/// unambiguous, or distro-info data is unavailable).
///
/// Examples:
/// - `"unstable"` → `Some("sid")`
/// - `"sid"` → `Some("unstable")`
/// - `"testing"` → `Some("forky")` (current testing codename)
/// - `"trixie"` → `Some("stable")` (current stable codename)
/// - `"experimental"` → `None`
pub fn get_distribution_mapping(distribution: &str) -> Option<&'static str> {
    let data = cached();

    match distribution {
        "unstable" => Some("sid"),
        "sid" => Some("unstable"),
        "testing" => data.testing.as_deref(),
        "stable" => data.stable.as_deref(),
        "oldstable" => data.oldstable.as_deref(),
        "experimental" => None,
        codename => {
            if data.testing.as_deref() == Some(codename) {
                Some("testing")
            } else if data.stable.as_deref() == Some(codename) {
                Some("stable")
            } else if data.oldstable.as_deref() == Some(codename) {
                Some("oldstable")
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unstable_maps_to_sid() {
        assert_eq!(get_distribution_mapping("unstable"), Some("sid"));
    }

    #[test]
    fn test_sid_maps_to_unstable() {
        assert_eq!(get_distribution_mapping("sid"), Some("unstable"));
    }

    #[test]
    fn test_experimental_has_no_mapping() {
        assert_eq!(get_distribution_mapping("experimental"), None);
    }

    #[test]
    fn test_testing_maps_to_codename() {
        let result = get_distribution_mapping("testing");
        if let Some(codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
        }
    }

    #[test]
    fn test_stable_maps_to_codename() {
        let result = get_distribution_mapping("stable");
        if let Some(codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
            assert_ne!(codename, "unstable");
        }
    }

    #[test]
    fn test_suite_codenames_are_distinct() {
        let data = cached();
        let mut seen = std::collections::HashSet::new();
        for name in [&data.testing, &data.stable, &data.oldstable]
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
        let dists = get_all_distributions();
        // Should have more than just the aliases (if distro-info is available)
        assert!(dists.len() > 7);
    }

    #[test]
    fn test_codename_reverse_mapping() {
        // If testing maps to a codename, that codename should map back to "testing"
        if let Some(codename) = get_distribution_mapping("testing") {
            assert_eq!(get_distribution_mapping(codename), Some("testing"));
        }
        if let Some(codename) = get_distribution_mapping("stable") {
            assert_eq!(get_distribution_mapping(codename), Some("stable"));
        }
        if let Some(codename) = get_distribution_mapping("oldstable") {
            assert_eq!(get_distribution_mapping(codename), Some("oldstable"));
        }
    }
}
