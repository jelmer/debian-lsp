use std::fs;
use std::sync::OnceLock;

use crate::deb822::completion::FieldInfo;

/// All available Debian copyright file fields (DEP-5 format)
///
/// Descriptions are sourced from the DEP-5 specification:
/// <https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/>
pub const COPYRIGHT_FIELDS: &[FieldInfo] = &[
    // Header paragraph fields
    FieldInfo::new(
        "Format",
        "URI of the format specification, e.g. `https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/`. Required in the header paragraph.",
    ),
    FieldInfo::new(
        "Upstream-Name",
        "The name upstream uses for the software. Optional, only used in the header paragraph.",
    ),
    FieldInfo::new(
        "Upstream-Contact",
        "The preferred address(es) to reach the upstream project. Typically an email address or URL. May be a line-based list. Optional, only used in the header paragraph.",
    ),
    FieldInfo::new(
        "Source",
        "An explanation of where the upstream source came from. Typically a URL. This field may be used to point at the upstream source code repository. Optional, only used in the header paragraph.",
    ),
    FieldInfo::new(
        "Disclaimer",
        "Free-form text field for non-free or contrib packages to state that they are not part of Debian and to explain why. Optional, only used in the header paragraph.",
    ),
    FieldInfo::new(
        "Comment",
        "Free-form text field for additional information. For example, it might quote the software's copyright notice if that triggers a specific requirement in the license.",
    ),
    FieldInfo::new(
        "License",
        "Short name of the license on the first line (the synopsis), followed by the full license text on subsequent lines. In a Files paragraph, the synopsis is required but the full text may be omitted if a stand-alone License paragraph with the same synopsis exists.",
    ),
    FieldInfo::new(
        "Copyright",
        "One or more free-form copyright statements, one per line. Required in Files paragraphs. In the header paragraph, it applies to files not matched by any Files paragraph.",
    ),
    FieldInfo::new(
        "Files-Excluded",
        "Whitespace-separated list of filename patterns indicating files to be excluded from the upstream source when repacking. Used by `uscan` when repacking upstream tarballs. Only used in the header paragraph.",
    ),
    // Files paragraph fields
    FieldInfo::new(
        "Files",
        "Whitespace-separated list of filename patterns indicating the files covered by this paragraph. Patterns use `fnmatch(3)` syntax (e.g. `*`, `?`, `[...]`). An asterisk (`*`) matches all files in the directory. Required in Files paragraphs.",
    ),
    // License paragraph has License and Comment which are already listed above
];

/// Get the standard casing for a field name
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    crate::deb822::completion::get_standard_field_name(COPYRIGHT_FIELDS, field_name)
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

        // Only check for licenses if /usr/share/common-licenses exists
        // On macOS/Windows this directory won't exist
        if std::path::Path::new("/usr/share/common-licenses").exists() {
            assert!(!licenses.is_empty());

            // Should have common licenses from /usr/share/common-licenses
            let license_strs: Vec<&str> = licenses.iter().map(|s| s.as_str()).collect();
            assert!(
                license_strs.contains(&"GPL-2") || license_strs.contains(&"Apache-2.0"),
                "Should contain common licenses"
            );
        }
    }

    #[test]
    fn test_load_common_licenses() {
        let licenses = load_common_licenses();

        // Only check for licenses if /usr/share/common-licenses exists
        // On macOS/Windows this directory won't exist
        if std::path::Path::new("/usr/share/common-licenses").exists() {
            assert!(!licenses.is_empty());

            for license in &licenses {
                assert!(!license.is_empty());
            }
        }
    }
}
