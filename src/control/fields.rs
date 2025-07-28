/// Debian control file field definitions
pub struct ControlField {
    pub name: &'static str,
    pub description: &'static str,
}

impl ControlField {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description }
    }
}

/// All available Debian control file fields
pub const CONTROL_FIELDS: &[ControlField] = &[
    ControlField::new("Source", "Name of the source package"),
    ControlField::new("Section", "Classification of the package"),
    ControlField::new("Priority", "Priority of the package"),
    ControlField::new("Maintainer", "Package maintainer's name and email"),
    ControlField::new("Uploaders", "Additional maintainers"),
    ControlField::new("Build-Depends", "Build dependencies"),
    ControlField::new(
        "Build-Depends-Indep",
        "Architecture-independent build dependencies",
    ),
    ControlField::new("Build-Conflicts", "Packages that conflict during build"),
    ControlField::new("Standards-Version", "Debian Policy version"),
    ControlField::new("Homepage", "Upstream project homepage"),
    ControlField::new("Vcs-Browser", "Web interface for VCS"),
    ControlField::new("Vcs-Git", "Git repository URL"),
    ControlField::new("Package", "Binary package name"),
    ControlField::new("Architecture", "Supported architectures"),
    ControlField::new("Multi-Arch", "Multi-architecture support"),
    ControlField::new("Depends", "Package dependencies"),
    ControlField::new("Pre-Depends", "Pre-installation dependencies"),
    ControlField::new("Recommends", "Recommended packages"),
    ControlField::new("Suggests", "Suggested packages"),
    ControlField::new("Enhances", "Packages enhanced by this one"),
    ControlField::new("Conflicts", "Conflicting packages"),
    ControlField::new("Breaks", "Packages broken by this one"),
    ControlField::new("Provides", "Virtual packages provided"),
    ControlField::new("Replaces", "Packages replaced by this one"),
    ControlField::new("Description", "Package description"),
    ControlField::new("Essential", "Essential package flag"),
    ControlField::new("Rules-Requires-Root", "Root privileges requirement"),
];

/// Get the standard casing for a field name
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    let lowercase = field_name.to_lowercase();
    for field in CONTROL_FIELDS {
        if field.name.to_lowercase() == lowercase {
            return Some(field.name);
        }
    }
    None
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
}
