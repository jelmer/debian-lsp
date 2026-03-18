//! Debian changelog field definitions and common values

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

/// Get Debian distribution names from distro-info-data
/// Returns a slice of distribution names (codenames and aliases)
pub fn get_debian_distributions() -> &'static [String] {
    crate::distros::get_all_distributions()
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
