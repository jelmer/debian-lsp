/// Debian watch file option definition
pub struct WatchOption {
    pub name: &'static str,
    pub description: &'static str,
    pub value_type: OptionValueType,
}

/// Type of value an option accepts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionValueType {
    /// Boolean option (no value)
    Boolean,
    /// String value
    String,
    /// Enum with predefined values
    Enum(&'static [&'static str]),
}

impl WatchOption {
    pub const fn new(
        name: &'static str,
        description: &'static str,
        value_type: OptionValueType,
    ) -> Self {
        Self {
            name,
            description,
            value_type,
        }
    }
}

/// All available Debian watch file options
pub const WATCH_OPTIONS: &[WatchOption] = &[
    WatchOption::new(
        "component",
        "Component name for multi-tarball packages",
        OptionValueType::String,
    ),
    WatchOption::new(
        "compression",
        "Compression format (gzip, xz, bzip2, lzma)",
        OptionValueType::Enum(&["gzip", "xz", "bzip2", "lzma", "default"]),
    ),
    WatchOption::new(
        "mode",
        "Download mode (lwp, git, svn)",
        OptionValueType::Enum(&["lwp", "git", "svn"]),
    ),
    WatchOption::new(
        "pgpmode",
        "PGP verification mode",
        OptionValueType::Enum(&[
            "auto", "default", "mangle", "next", "previous", "self", "gittag",
        ]),
    ),
    WatchOption::new(
        "searchmode",
        "Search mode for finding upstream versions",
        OptionValueType::Enum(&["html", "plain"]),
    ),
    WatchOption::new(
        "gitmode",
        "Git clone mode",
        OptionValueType::Enum(&["shallow", "full"]),
    ),
    WatchOption::new(
        "gitexport",
        "Git export mode",
        OptionValueType::Enum(&["default", "all"]),
    ),
    WatchOption::new(
        "pretty",
        "Pretty format for git tags",
        OptionValueType::String,
    ),
    WatchOption::new(
        "uversionmangle",
        "Upstream version mangling rules (s/pattern/replacement/)",
        OptionValueType::String,
    ),
    WatchOption::new(
        "oversionmangle",
        "Upstream version mangling rules (alternative name)",
        OptionValueType::String,
    ),
    WatchOption::new(
        "dversionmangle",
        "Debian version mangling rules (s/pattern/replacement/)",
        OptionValueType::String,
    ),
    WatchOption::new(
        "dirversionmangle",
        "Directory version mangling rules for mode=git",
        OptionValueType::String,
    ),
    WatchOption::new(
        "pagemangle",
        "Page content mangling rules",
        OptionValueType::String,
    ),
    WatchOption::new(
        "downloadurlmangle",
        "Download URL mangling rules",
        OptionValueType::String,
    ),
    WatchOption::new(
        "pgpsigurlmangle",
        "PGP signature URL mangling rules",
        OptionValueType::String,
    ),
    WatchOption::new(
        "filenamemangle",
        "Filename mangling rules",
        OptionValueType::String,
    ),
    WatchOption::new(
        "versionmangle",
        "Version policy (debian, same, previous, ignore, group, checksum)",
        OptionValueType::String,
    ),
    WatchOption::new(
        "user-agent",
        "User agent string for HTTP requests",
        OptionValueType::String,
    ),
    WatchOption::new(
        "useragent",
        "User agent string for HTTP requests (alternative name)",
        OptionValueType::String,
    ),
    WatchOption::new(
        "ctype",
        "Component type (perl, nodejs)",
        OptionValueType::Enum(&["perl", "nodejs"]),
    ),
    WatchOption::new(
        "repacksuffix",
        "Suffix for repacked tarballs",
        OptionValueType::String,
    ),
    WatchOption::new(
        "decompress",
        "Decompress downloaded files",
        OptionValueType::Boolean,
    ),
    WatchOption::new(
        "bare",
        "Use bare git clone for mode=git",
        OptionValueType::Boolean,
    ),
    WatchOption::new(
        "repack",
        "Repack the upstream tarball",
        OptionValueType::Boolean,
    ),
];

/// Watch file format versions
pub const WATCH_VERSIONS: &[u32] = &[1, 2, 3, 4, 5];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_options() {
        assert!(!WATCH_OPTIONS.is_empty());
        assert!(WATCH_OPTIONS.len() >= 20);

        // Test specific options exist
        let option_names: Vec<_> = WATCH_OPTIONS.iter().map(|o| o.name).collect();
        assert!(option_names.contains(&"mode"));
        assert!(option_names.contains(&"pgpmode"));
        assert!(option_names.contains(&"uversionmangle"));
        assert!(option_names.contains(&"compression"));
    }

    #[test]
    fn test_watch_option_validity() {
        for option in WATCH_OPTIONS {
            assert!(!option.name.is_empty());
            assert!(!option.description.is_empty());

            // Check that enum options have values
            if let OptionValueType::Enum(values) = option.value_type {
                assert!(
                    !values.is_empty(),
                    "Enum option {} has no values",
                    option.name
                );
            }
        }
    }

    #[test]
    fn test_watch_versions() {
        assert_eq!(WATCH_VERSIONS, &[1, 2, 3, 4, 5]);
    }
}
