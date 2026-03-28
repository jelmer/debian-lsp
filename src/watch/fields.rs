/// Type of value a watch field/option accepts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionValueType {
    /// Boolean option (no value)
    Boolean,
    /// String value
    String,
    /// Enum with predefined values
    Enum(&'static [&'static str]),
}

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

/// A watch file field definition, used for both v1-4 line-based options and v5 deb822 fields.
pub struct WatchField {
    /// Canonical field name (deb822 / title-case form, used in v5).
    pub deb822_name: &'static str,
    /// Option name used in v1-4 line-based format, or `None` for v5-only fields.
    pub linebased_name: Option<&'static str>,
    /// Human-readable description.
    pub description: &'static str,
    /// Type of value the field accepts.
    pub value_type: OptionValueType,
    /// Callback returning completion items for this field's values.
    /// Receives the prefix already typed by the user for filtering.
    pub complete_values: fn(&str) -> Vec<CompletionItem>,
}

fn no_completions(_prefix: &str) -> Vec<CompletionItem> {
    vec![]
}

fn enum_completions(values: &[&str], prefix: &str) -> Vec<CompletionItem> {
    let normalized = prefix.trim().to_ascii_lowercase();
    values
        .iter()
        .filter(|v| v.starts_with(&normalized))
        .map(|&v| CompletionItem {
            label: v.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            ..Default::default()
        })
        .collect()
}

fn boolean_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["yes", "no"], prefix)
}

fn compression_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["gzip", "xz", "bzip2", "lzma", "default"], prefix)
}

fn mode_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["lwp", "git", "svn"], prefix)
}

fn pgpmode_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(
        &[
            "auto", "default", "mangle", "next", "previous", "self", "gittag",
        ],
        prefix,
    )
}

fn searchmode_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["html", "plain"], prefix)
}

fn gitmode_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["shallow", "full"], prefix)
}

fn gitexport_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["default", "all"], prefix)
}

fn ctype_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(&["perl", "nodejs"], prefix)
}

// TODO: derive template names from debian_watch::templates::Template enum
fn template_completions(prefix: &str) -> Vec<CompletionItem> {
    enum_completions(
        &["github", "gitlab", "pypi", "npmregistry", "metacpan"],
        prefix,
    )
}

impl WatchField {
    pub const fn new(
        deb822_name: &'static str,
        linebased_name: Option<&'static str>,
        description: &'static str,
        value_type: OptionValueType,
        complete_values: fn(&str) -> Vec<CompletionItem>,
    ) -> Self {
        Self {
            deb822_name,
            linebased_name,
            description,
            value_type,
            complete_values,
        }
    }
}

/// All known watch file fields/options.
pub const WATCH_FIELDS: &[WatchField] = &[
    // v5-only fields (no line-based equivalent)
    WatchField::new(
        "Version",
        None,
        "Watch file format version",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Source",
        None,
        "URL to check for upstream releases",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Matching-Pattern",
        None,
        "Regex pattern to match upstream files",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Template",
        None,
        "URL template for constructing download URLs (github, gitlab, pypi, npmregistry, metacpan)",
        OptionValueType::Enum(&["github", "gitlab", "pypi", "npmregistry", "metacpan"]),
        template_completions,
    ),
    WatchField::new(
        "Owner",
        None,
        "Owner name for repository-based sources (used with github template)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Project",
        None,
        "Project name for repository-based sources (used with github template)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Dist",
        None,
        "Distribution or package name (used with gitlab, pypi, npmregistry, metacpan templates)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Release-Only",
        None,
        "Restrict to releases only, not all tags (used with github and gitlab templates)",
        OptionValueType::Boolean,
        boolean_completions,
    ),
    WatchField::new(
        "Version-Type",
        None,
        "Version pattern type (e.g. semantic, stable) — expands to @TYPE_VERSION@ in matching pattern",
        OptionValueType::String,
        no_completions,
    ),
    // Fields available in both v1-4 (as options) and v5 (as deb822 fields)
    WatchField::new(
        "Component",
        Some("component"),
        "Component name for multi-tarball packages",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Compression",
        Some("compression"),
        "Compression format (gzip, xz, bzip2, lzma)",
        OptionValueType::Enum(&["gzip", "xz", "bzip2", "lzma", "default"]),
        compression_completions,
    ),
    WatchField::new(
        "Mode",
        Some("mode"),
        "Download mode (lwp, git, svn)",
        OptionValueType::Enum(&["lwp", "git", "svn"]),
        mode_completions,
    ),
    WatchField::new(
        "Pgpmode",
        Some("pgpmode"),
        "PGP verification mode",
        OptionValueType::Enum(&[
            "auto", "default", "mangle", "next", "previous", "self", "gittag",
        ]),
        pgpmode_completions,
    ),
    WatchField::new(
        "Searchmode",
        Some("searchmode"),
        "Search mode for finding upstream versions",
        OptionValueType::Enum(&["html", "plain"]),
        searchmode_completions,
    ),
    WatchField::new(
        "Gitmode",
        Some("gitmode"),
        "Git clone mode",
        OptionValueType::Enum(&["shallow", "full"]),
        gitmode_completions,
    ),
    WatchField::new(
        "Gitexport",
        Some("gitexport"),
        "Git export mode",
        OptionValueType::Enum(&["default", "all"]),
        gitexport_completions,
    ),
    WatchField::new(
        "Pretty",
        Some("pretty"),
        "Pretty format for git tags",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Uversionmangle",
        Some("uversionmangle"),
        "Upstream version mangling rules (s/pattern/replacement/)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Oversionmangle",
        Some("oversionmangle"),
        "Upstream version mangling rules (alternative name)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Dversionmangle",
        Some("dversionmangle"),
        "Debian version mangling rules (s/pattern/replacement/)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Dirversionmangle",
        Some("dirversionmangle"),
        "Directory version mangling rules for mode=git",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Pagemangle",
        Some("pagemangle"),
        "Page content mangling rules",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Downloadurlmangle",
        Some("downloadurlmangle"),
        "Download URL mangling rules",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Pgpsigurlmangle",
        Some("pgpsigurlmangle"),
        "PGP signature URL mangling rules",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Filenamemangle",
        Some("filenamemangle"),
        "Filename mangling rules",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Versionmangle",
        Some("versionmangle"),
        "Version policy (debian, same, previous, ignore, group, checksum)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "User-Agent",
        Some("user-agent"),
        "User agent string for HTTP requests",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Useragent",
        Some("useragent"),
        "User agent string for HTTP requests (alternative name)",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Ctype",
        Some("ctype"),
        "Component type (perl, nodejs)",
        OptionValueType::Enum(&["perl", "nodejs"]),
        ctype_completions,
    ),
    WatchField::new(
        "Repacksuffix",
        Some("repacksuffix"),
        "Suffix for repacked tarballs",
        OptionValueType::String,
        no_completions,
    ),
    WatchField::new(
        "Decompress",
        Some("decompress"),
        "Decompress downloaded files",
        OptionValueType::Boolean,
        boolean_completions,
    ),
    WatchField::new(
        "Bare",
        Some("bare"),
        "Use bare git clone for mode=git",
        OptionValueType::Boolean,
        boolean_completions,
    ),
    WatchField::new(
        "Repack",
        Some("repack"),
        "Repack the upstream tarball",
        OptionValueType::Boolean,
        boolean_completions,
    ),
];

/// Get the standard (canonical deb822) name for a watch field.
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    let lower = field_name.to_lowercase();
    WATCH_FIELDS
        .iter()
        .find(|f| f.deb822_name.to_lowercase() == lower)
        .map(|f| f.deb822_name)
}

/// Watch file format versions
pub const WATCH_VERSIONS: &[u32] = &[1, 2, 3, 4, 5];

/// Line-based watch file format versions (v5 uses deb822 format)
pub const WATCH_LINEBASED_VERSIONS: &[u32] = &[1, 2, 3, 4];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_fields() {
        assert!(!WATCH_FIELDS.is_empty());
        assert!(WATCH_FIELDS.len() >= 20);

        let deb822_names: Vec<_> = WATCH_FIELDS.iter().map(|f| f.deb822_name).collect();
        assert!(deb822_names.contains(&"Mode"));
        assert!(deb822_names.contains(&"Pgpmode"));
        assert!(deb822_names.contains(&"Uversionmangle"));
        assert!(deb822_names.contains(&"Compression"));
        assert!(deb822_names.contains(&"Source"));
        assert!(deb822_names.contains(&"Matching-Pattern"));
        assert!(deb822_names.contains(&"Version"));
    }

    #[test]
    fn test_linebased_options() {
        let options: Vec<_> = WATCH_FIELDS
            .iter()
            .filter_map(|f| f.linebased_name)
            .collect();
        assert!(options.len() >= 20);
        assert!(options.contains(&"mode"));
        assert!(options.contains(&"pgpmode"));
        assert!(options.contains(&"uversionmangle"));
        assert!(options.contains(&"compression"));
    }

    #[test]
    fn test_v5_only_fields_have_no_linebased_name() {
        for field in WATCH_FIELDS {
            if [
                "Version",
                "Source",
                "Matching-Pattern",
                "Template",
                "Owner",
                "Project",
                "Dist",
                "Release-Only",
                "Version-Type",
            ]
            .contains(&field.deb822_name)
            {
                assert!(
                    field.linebased_name.is_none(),
                    "{} should be v5-only",
                    field.deb822_name
                );
            }
        }
    }

    #[test]
    fn test_watch_field_validity() {
        for field in WATCH_FIELDS {
            assert!(!field.deb822_name.is_empty());
            assert!(!field.description.is_empty());

            if let OptionValueType::Enum(values) = field.value_type {
                assert!(
                    !values.is_empty(),
                    "Enum field {} has no values",
                    field.deb822_name
                );
            }
        }
    }

    #[test]
    fn test_get_standard_field_name() {
        assert_eq!(get_standard_field_name("Source"), Some("Source"));
        assert_eq!(get_standard_field_name("source"), Some("Source"));
        assert_eq!(
            get_standard_field_name("Matching-Pattern"),
            Some("Matching-Pattern")
        );
        assert_eq!(get_standard_field_name("mode"), Some("Mode"));
        assert_eq!(get_standard_field_name("UnknownField"), None);
    }

    #[test]
    fn test_watch_versions() {
        assert_eq!(WATCH_VERSIONS, &[1, 2, 3, 4, 5]);
    }
}
