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
/// Each entry is (value, description).
pub const CONTROL_PRIORITY_VALUES: &[(&str, &str)] = &[
    ("required", "Essential for the system to function"),
    ("important", "Important programs, including those expected on a Unix-like system"),
    ("standard", "Reasonably small but not too limited character-mode system"),
    ("optional", "All packages not required for a reasonably functional system"),
    ("extra", "Deprecated alias for optional"),
];

/// Debian policy section values for normal packages.
/// Each entry is (value, description).
// TODO: Read section list from an external file or Debian policy data instead of hardcoding.
pub const CONTROL_SECTION_VALUES: &[(&str, &str)] = &[
    ("admin", "System administration utilities"),
    ("cli-mono", "Mono/CLI based programs"),
    ("comm", "Communication programs"),
    ("database", "Database servers and tools"),
    ("debug", "Debug packages"),
    ("devel", "Development tools and libraries"),
    ("doc", "Documentation"),
    ("editors", "Text editors"),
    ("education", "Educational software"),
    ("electronics", "Electronics and electrical engineering"),
    ("embedded", "Embedded systems software"),
    ("fonts", "Font packages"),
    ("games", "Games and amusements"),
    ("gnome", "GNOME desktop environment"),
    ("gnu-r", "GNU R statistical system"),
    ("gnustep", "GNUstep environment"),
    ("graphics", "Graphics tools"),
    ("hamradio", "Ham radio software"),
    ("haskell", "Haskell programming language"),
    ("httpd", "Web servers"),
    ("interpreters", "Interpreted languages"),
    ("introspection", "GObject introspection data"),
    ("java", "Java programming language"),
    ("javascript", "JavaScript programming"),
    ("kde", "KDE desktop environment"),
    ("kernel", "Kernel and kernel modules"),
    ("libdevel", "Development libraries"),
    ("libs", "Shared libraries"),
    ("lisp", "Lisp programming language"),
    ("localization", "Localization and internationalization"),
    ("mail", "Email programs"),
    ("math", "Mathematics and numerical computation"),
    ("metapackages", "Metapackages"),
    ("misc", "Miscellaneous"),
    ("net", "Networking tools"),
    ("news", "Usenet news"),
    ("ocaml", "OCaml programming language"),
    ("oldlibs", "Obsolete libraries"),
    ("otherosfs", "Other OS file systems"),
    ("perl", "Perl programming language"),
    ("php", "PHP programming language"),
    ("python", "Python programming language"),
    ("ruby", "Ruby programming language"),
    ("rust", "Rust programming language"),
    ("science", "Scientific software"),
    ("shells", "Command-line shells"),
    ("sound", "Sound and audio"),
    ("tasks", "Task packages for installation"),
    ("tex", "TeX typesetting system"),
    ("text", "Text processing utilities"),
    ("utils", "General-purpose utilities"),
    ("vcs", "Version control systems"),
    ("video", "Video tools"),
    ("web", "Web browsers and tools"),
    ("x11", "X Window System"),
    ("xfce", "Xfce desktop environment"),
    ("zope", "Zope/Plone framework"),
];

/// Debian archive areas used as section prefixes in control fields.
///
/// Section field values can be `area/section` for non-main archive areas.
pub const CONTROL_SECTION_AREAS: &[&str] = &["contrib", "non-free", "non-free-firmware"];

/// Debian policy special section values.
///
/// `debian-installer` is used for installer packages and not normal packages.
pub const CONTROL_SPECIAL_SECTION_VALUES: &[(&str, &str)] =
    &[("debian-installer", "Debian installer components")];

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
        let names: Vec<_> = CONTROL_PRIORITY_VALUES.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            &["required", "important", "standard", "optional", "extra"]
        );
        for (_, desc) in CONTROL_PRIORITY_VALUES {
            assert!(!desc.is_empty());
        }
    }

    #[test]
    fn test_control_section_values() {
        let names: Vec<_> = CONTROL_SECTION_VALUES.iter().map(|(n, _)| *n).collect();
        assert!(!names.is_empty());
        assert!(names.contains(&"admin"));
        assert!(names.contains(&"python"));
        assert!(names.contains(&"xfce"));
        assert!(!names.contains(&"debian-installer"));
        for (_, desc) in CONTROL_SECTION_VALUES {
            assert!(!desc.is_empty());
        }
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
        let names: Vec<_> = CONTROL_SPECIAL_SECTION_VALUES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, &["debian-installer"]);
    }
}
