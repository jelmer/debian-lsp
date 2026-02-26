use std::fs;
use std::sync::OnceLock;

/// Debian copyright file field definitions
pub struct CopyrightField {
    pub name: &'static str,
    pub description: &'static str,
}

impl CopyrightField {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description }
    }
}

/// All available Debian copyright file fields (DEP-5 format)
pub const COPYRIGHT_FIELDS: &[CopyrightField] = &[
    // Header paragraph fields
    CopyrightField::new("Format", "URI of the format specification (DEP-5)"),
    CopyrightField::new("Upstream-Name", "Name of the upstream project"),
    CopyrightField::new("Upstream-Contact", "Contact information for upstream"),
    CopyrightField::new("Source", "Where the upstream source can be obtained"),
    CopyrightField::new("Disclaimer", "Disclaimer for non-free packages"),
    CopyrightField::new("Comment", "Additional information or context"),
    CopyrightField::new("License", "License for the work"),
    CopyrightField::new("Copyright", "Copyright holder information"),
    CopyrightField::new("Files-Excluded", "Files excluded from the source package"),
    // Files paragraph fields
    CopyrightField::new("Files", "File patterns covered by this paragraph"),
    // License paragraph has License and Comment which are already listed above
];

/// Get the standard casing for a field name
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    let lowercase = field_name.to_lowercase();
    for field in COPYRIGHT_FIELDS {
        if field.name.to_lowercase() == lowercase {
            return Some(field.name);
        }
    }
    None
}

/// Cache for common license names loaded from the system
static COMMON_LICENSES_CACHE: OnceLock<Vec<String>> = OnceLock::new();

/// Load common license names from /usr/share/common-licenses
fn load_common_licenses() -> Vec<String> {
    const COMMON_LICENSES_DIR: &str = "/usr/share/common-licenses";

    let mut licenses = Vec::new();

    if let Ok(entries) = fs::read_dir(COMMON_LICENSES_DIR) {
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                // Skip symlinks that are just shortcuts (GPL, LGPL, GFDL without version)
                if entry.path().is_symlink() {
                    continue;
                }
                licenses.push(file_name);
            }
        }
    }

    licenses.sort();
    licenses
}

/// Get common license names (cached)
pub fn get_common_licenses() -> &'static [String] {
    COMMON_LICENSES_CACHE.get_or_init(load_common_licenses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copyright_fields() {
        assert!(!COPYRIGHT_FIELDS.is_empty());
        assert!(COPYRIGHT_FIELDS.len() >= 10);

        // Test specific fields exist
        let field_names: Vec<_> = COPYRIGHT_FIELDS.iter().map(|f| f.name).collect();
        assert!(field_names.contains(&"Format"));
        assert!(field_names.contains(&"Files"));
        assert!(field_names.contains(&"License"));
        assert!(field_names.contains(&"Copyright"));
    }

    #[test]
    fn test_copyright_field_validity() {
        for field in COPYRIGHT_FIELDS {
            assert!(!field.name.is_empty());
            assert!(!field.description.is_empty());
            assert!(
                field
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "Field {} contains invalid characters",
                field.name
            );
        }
    }

    #[test]
    fn test_get_standard_field_name() {
        // Test correct casing - should return the same
        assert_eq!(get_standard_field_name("Format"), Some("Format"));
        assert_eq!(get_standard_field_name("Files"), Some("Files"));
        assert_eq!(get_standard_field_name("License"), Some("License"));

        // Test incorrect casing - should return the standard form
        assert_eq!(get_standard_field_name("format"), Some("Format"));
        assert_eq!(get_standard_field_name("files"), Some("Files"));
        assert_eq!(get_standard_field_name("license"), Some("License"));
        assert_eq!(get_standard_field_name("COPYRIGHT"), Some("Copyright"));

        // Test unknown fields - should return None
        assert_eq!(get_standard_field_name("UnknownField"), None);
        assert_eq!(get_standard_field_name("random"), None);
    }

    #[test]
    fn test_get_common_licenses() {
        let licenses = get_common_licenses();
        assert!(!licenses.is_empty());

        // Should have common licenses from /usr/share/common-licenses
        let license_strs: Vec<&str> = licenses.iter().map(|s| s.as_str()).collect();
        assert!(
            license_strs.contains(&"GPL-2") || license_strs.contains(&"Apache-2.0"),
            "Should contain common licenses"
        );
    }

    #[test]
    fn test_load_common_licenses() {
        let licenses = load_common_licenses();
        assert!(!licenses.is_empty());

        for license in &licenses {
            assert!(!license.is_empty());
        }
    }
}
