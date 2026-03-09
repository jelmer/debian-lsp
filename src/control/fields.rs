use crate::deb822::completion::FieldInfo;

/// All available Debian control file fields
pub const CONTROL_FIELDS: &[FieldInfo] = &[
    FieldInfo::new("Source", "Name of the source package"),
    FieldInfo::new("Section", "Classification of the package"),
    FieldInfo::new("Priority", "Priority of the package"),
    FieldInfo::new("Maintainer", "Package maintainer's name and email"),
    FieldInfo::new("Uploaders", "Additional maintainers"),
    FieldInfo::new("Build-Depends", "Build dependencies"),
    FieldInfo::new(
        "Build-Depends-Indep",
        "Architecture-independent build dependencies",
    ),
    FieldInfo::new("Build-Conflicts", "Packages that conflict during build"),
    FieldInfo::new("Standards-Version", "Debian Policy version"),
    FieldInfo::new("Homepage", "Upstream project homepage"),
    FieldInfo::new("Vcs-Browser", "Web interface for VCS"),
    FieldInfo::new("Vcs-Git", "Git repository URL"),
    FieldInfo::new("Package", "Binary package name"),
    FieldInfo::new("Architecture", "Supported architectures"),
    FieldInfo::new("Multi-Arch", "Multi-architecture support"),
    FieldInfo::new("Depends", "Package dependencies"),
    FieldInfo::new("Pre-Depends", "Pre-installation dependencies"),
    FieldInfo::new("Recommends", "Recommended packages"),
    FieldInfo::new("Suggests", "Suggested packages"),
    FieldInfo::new("Enhances", "Packages enhanced by this one"),
    FieldInfo::new("Conflicts", "Conflicting packages"),
    FieldInfo::new("Breaks", "Packages broken by this one"),
    FieldInfo::new("Provides", "Virtual packages provided"),
    FieldInfo::new("Replaces", "Packages replaced by this one"),
    FieldInfo::new("Description", "Package description"),
    FieldInfo::new("Essential", "Essential package flag"),
    FieldInfo::new("Rules-Requires-Root", "Root privileges requirement"),
];

/// Get the standard casing for a field name
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    crate::deb822::completion::get_standard_field_name(CONTROL_FIELDS, field_name)
}

/// Common package names for completion
pub const COMMON_PACKAGES: &[&str] = &[
    "debhelper-compat",
    "dh-python",
    "python3-all",
    "python3-setuptools",
    "cmake",
    "pkg-config",
    "libssl-dev",
    "libc6-dev",
];

/// Debian policy-recognized priority values for control files.
pub const CONTROL_PRIORITY_VALUES: &[&str] =
    &["required", "important", "standard", "optional", "extra"];

/// Debian policy section values for normal packages.
pub const CONTROL_SECTION_VALUES: &[&str] = &[
    "admin",
    "cli-mono",
    "comm",
    "database",
    "debug",
    "devel",
    "doc",
    "editors",
    "education",
    "electronics",
    "embedded",
    "fonts",
    "games",
    "gnome",
    "gnu-r",
    "gnustep",
    "graphics",
    "hamradio",
    "haskell",
    "httpd",
    "interpreters",
    "introspection",
    "java",
    "javascript",
    "kde",
    "kernel",
    "libdevel",
    "libs",
    "lisp",
    "localization",
    "mail",
    "math",
    "metapackages",
    "misc",
    "net",
    "news",
    "ocaml",
    "oldlibs",
    "otherosfs",
    "perl",
    "php",
    "python",
    "ruby",
    "rust",
    "science",
    "shells",
    "sound",
    "tasks",
    "tex",
    "text",
    "utils",
    "vcs",
    "video",
    "web",
    "x11",
    "xfce",
    "zope",
];

/// Debian archive areas used as section prefixes in control fields.
///
/// Section field values can be `area/section` for non-main archive areas.
pub const CONTROL_SECTION_AREAS: &[&str] = &["contrib", "non-free", "non-free-firmware"];

/// Debian policy special section values.
///
/// `debian-installer` is used for installer packages and not normal packages.
pub const CONTROL_SPECIAL_SECTION_VALUES: &[&str] = &["debian-installer"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_fields() {
        assert!(!CONTROL_FIELDS.is_empty());
        assert!(CONTROL_FIELDS.len() >= 20);

        // Test specific fields exist
        let field_names: Vec<_> = CONTROL_FIELDS.iter().map(|f| f.name).collect();
        assert!(field_names.contains(&"Source"));
        assert!(field_names.contains(&"Package"));
        assert!(field_names.contains(&"Depends"));
        assert!(field_names.contains(&"Build-Depends"));
    }

    #[test]
    fn test_control_field_validity() {
        for field in CONTROL_FIELDS {
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
    fn test_common_packages() {
        assert!(!COMMON_PACKAGES.is_empty());

        for package in COMMON_PACKAGES {
            assert!(!package.is_empty());
            assert!(
                package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c.is_ascii_digit()),
                "Package {} contains invalid characters",
                package
            );
        }
    }

    #[test]
    fn test_control_priority_values() {
        assert_eq!(
            CONTROL_PRIORITY_VALUES,
            &["required", "important", "standard", "optional", "extra"]
        );
    }

    #[test]
    fn test_control_section_values() {
        assert!(!CONTROL_SECTION_VALUES.is_empty());
        assert!(CONTROL_SECTION_VALUES.contains(&"admin"));
        assert!(CONTROL_SECTION_VALUES.contains(&"python"));
        assert!(CONTROL_SECTION_VALUES.contains(&"xfce"));
        assert!(!CONTROL_SECTION_VALUES.contains(&"debian-installer"));
    }

    #[test]
    fn test_control_section_areas() {
        assert_eq!(
            CONTROL_SECTION_AREAS,
            &["contrib", "non-free", "non-free-firmware"]
        );
    }

    #[test]
    fn test_control_special_section_values() {
        assert_eq!(CONTROL_SPECIAL_SECTION_VALUES, &["debian-installer"]);
    }
}
