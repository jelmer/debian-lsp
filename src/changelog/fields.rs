/// Debian changelog field definitions and common values
use distro_info::{DebianDistroInfo, DistroInfo};

/// Debian urgency levels for changelog entries
pub struct UrgencyLevel {
    pub name: &'static str,
    pub description: &'static str,
}

impl UrgencyLevel {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description }
    }
}

/// All available urgency levels
pub const URGENCY_LEVELS: &[UrgencyLevel] = &[
    UrgencyLevel::new("low", "Low urgency update"),
    UrgencyLevel::new("medium", "Medium urgency update"),
    UrgencyLevel::new("high", "High urgency update"),
    UrgencyLevel::new("critical", "Critical urgency update"),
    UrgencyLevel::new("emergency", "Emergency urgency update"),
];

/// Common changelog entry prefixes
pub const CHANGELOG_PREFIXES: &[&str] = &["* ", "  + ", "  - ", "    - "];

/// Get the standard casing for an urgency level
pub fn get_standard_urgency_name(urgency: &str) -> Option<&'static str> {
    let lowercase = urgency.to_lowercase();
    for level in URGENCY_LEVELS {
        if level.name.to_lowercase() == lowercase {
            return Some(level.name);
        }
    }
    None
}

/// Get Debian distribution names from distro-info-data
/// Returns a vector of distribution names (codenames and aliases)
pub fn get_debian_distributions() -> Vec<String> {
    // Add common aliases first
    let mut distributions = vec![
        "unstable".to_string(),
        "stable".to_string(),
        "testing".to_string(),
        "oldstable".to_string(),
        "experimental".to_string(),
        "sid".to_string(),
        "UNRELEASED".to_string(),
    ];

    // Try to get distribution data from distro-info
    if let Ok(debian_info) = DebianDistroInfo::new() {
        // Add all release codenames
        for release in debian_info.iter() {
            let series = release.series();
            if !distributions.contains(&series.to_string()) {
                distributions.push(series.to_string());
            }
        }
    }

    distributions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urgency_levels() {
        assert!(!URGENCY_LEVELS.is_empty());
        assert_eq!(URGENCY_LEVELS.len(), 5);

        let urgency_names: Vec<_> = URGENCY_LEVELS.iter().map(|u| u.name).collect();
        assert!(urgency_names.contains(&"low"));
        assert!(urgency_names.contains(&"medium"));
        assert!(urgency_names.contains(&"high"));
        assert!(urgency_names.contains(&"critical"));
        assert!(urgency_names.contains(&"emergency"));
    }

    #[test]
    fn test_urgency_level_validity() {
        for level in URGENCY_LEVELS {
            assert!(!level.name.is_empty());
            assert!(!level.description.is_empty());
            assert!(
                level.name.chars().all(|c| c.is_ascii_lowercase()),
                "Urgency level {} should be lowercase",
                level.name
            );
        }
    }

    #[test]
    fn test_get_standard_urgency_name() {
        assert_eq!(get_standard_urgency_name("low"), Some("low"));
        assert_eq!(get_standard_urgency_name("LOW"), Some("low"));
        assert_eq!(get_standard_urgency_name("Low"), Some("low"));
        assert_eq!(get_standard_urgency_name("medium"), Some("medium"));
        assert_eq!(get_standard_urgency_name("MEDIUM"), Some("medium"));
        assert_eq!(get_standard_urgency_name("invalid"), None);
    }

    #[test]
    fn test_changelog_prefixes() {
        assert!(!CHANGELOG_PREFIXES.is_empty());
        assert!(CHANGELOG_PREFIXES.contains(&"* "));
        assert!(CHANGELOG_PREFIXES.contains(&"  + "));
    }

    #[test]
    fn test_get_debian_distributions() {
        let distributions = get_debian_distributions();
        assert!(!distributions.is_empty());

        // Check that common aliases are present
        assert!(distributions.contains(&"unstable".to_string()));
        assert!(distributions.contains(&"stable".to_string()));
        assert!(distributions.contains(&"testing".to_string()));
        assert!(distributions.contains(&"UNRELEASED".to_string()));
        assert!(distributions.contains(&"sid".to_string()));
    }
}
