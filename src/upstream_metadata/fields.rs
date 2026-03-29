use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

/// The type of value a DEP-12 field accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValueType {
    /// A plain scalar value (URL, string, etc.)
    Scalar,
    /// A sequence of scalar values (e.g. list of URLs)
    ScalarList,
    /// A sequence of mappings with known sub-fields
    MappingList(&'static [SubField]),
}

/// A sub-field within a mapping list value (e.g. Registry → Name, Entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubField {
    pub name: &'static str,
    pub description: &'static str,
    pub known_values: &'static [&'static str],
}

/// A field definition for the upstream/metadata file (DEP-12).
pub struct UpstreamField {
    pub name: &'static str,
    pub description: &'static str,
    pub value_type: FieldValueType,
}

impl UpstreamField {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            value_type: FieldValueType::Scalar,
        }
    }

    pub const fn with_value_type(
        name: &'static str,
        description: &'static str,
        value_type: FieldValueType,
    ) -> Self {
        Self {
            name,
            description,
            value_type,
        }
    }
}

/// Known registry names for the Registry field.
pub const KNOWN_REGISTRIES: &[&str] = &[
    "ASCL",
    "BitBucket",
    "CPAN",
    "Codeberg",
    "SourceForge",
    "GitHub",
    "GitLab",
    "Go",
    "Hackage",
    "Heptapod",
    "Launchpad",
    "Maven",
    "PyPI",
    "Savannah",
    "SourceHut",
    "crates.io",
    "npm",
];

/// Sub-fields for Registry entries.
pub const REGISTRY_SUBFIELDS: &[SubField] = &[
    SubField {
        name: "Name",
        description: "Name of the software registry",
        known_values: KNOWN_REGISTRIES,
    },
    SubField {
        name: "Entry",
        description: "Identifier or URL of the entry in the registry",
        known_values: &[],
    },
];

/// Known reference types for the Reference field.
pub const KNOWN_REFERENCE_TYPES: &[&str] = &[
    "Article",
    "Book",
    "Conference",
    "InProceedings",
    "Manual",
    "PhdThesis",
    "TechReport",
    "Unpublished",
];

/// Sub-fields for Reference entries.
pub const REFERENCE_SUBFIELDS: &[SubField] = &[
    SubField {
        name: "Type",
        description: "Type of bibliographic reference",
        known_values: KNOWN_REFERENCE_TYPES,
    },
    SubField {
        name: "Title",
        description: "Title of the referenced work",
        known_values: &[],
    },
    SubField {
        name: "Author",
        description: "Author(s) of the referenced work",
        known_values: &[],
    },
    SubField {
        name: "Year",
        description: "Publication year",
        known_values: &[],
    },
    SubField {
        name: "DOI",
        description: "Digital Object Identifier",
        known_values: &[],
    },
    SubField {
        name: "URL",
        description: "URL of the referenced work",
        known_values: &[],
    },
    SubField {
        name: "Journal",
        description: "Journal name",
        known_values: &[],
    },
    SubField {
        name: "Volume",
        description: "Volume number",
        known_values: &[],
    },
    SubField {
        name: "EPRINT",
        description: "arXiv or other e-print identifier",
        known_values: &[],
    },
    SubField {
        name: "ISSN",
        description: "International Standard Serial Number",
        known_values: &[],
    },
    SubField {
        name: "Comment",
        description: "Additional comments about the reference",
        known_values: &[],
    },
];

/// Sub-fields for Funding entries.
pub const FUNDING_SUBFIELDS: &[SubField] = &[
    SubField {
        name: "Type",
        description: "Type of funding source",
        known_values: &[],
    },
    SubField {
        name: "Funder",
        description: "Name of the funding organization",
        known_values: &[],
    },
    SubField {
        name: "Grant",
        description: "Grant identifier or number",
        known_values: &[],
    },
    SubField {
        name: "URL",
        description: "URL with more information about the funding",
        known_values: &[],
    },
];

use tower_lsp_server::ls_types::InsertTextFormat;

fn make_indent(indent: u32) -> String {
    " ".repeat(indent as usize)
}

fn enum_completions(values: &[&str], prefix: &str, indent: u32) -> Vec<CompletionItem> {
    let normalized = prefix.trim().to_ascii_lowercase();
    let indent_str = make_indent(indent);
    values
        .iter()
        .filter(|v| v.to_ascii_lowercase().starts_with(&normalized))
        .map(|&v| CompletionItem {
            label: v.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            insert_text: Some(format!("{v}\n{indent_str}$0")),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

/// Get value completions for a sub-field within a mapping list.
pub fn get_subfield_value_completions(
    subfields: &[SubField],
    subfield_name: &str,
    prefix: &str,
    indent: u32,
) -> Vec<CompletionItem> {
    let lower = subfield_name.to_ascii_lowercase();
    subfields
        .iter()
        .find(|sf| sf.name.to_ascii_lowercase() == lower)
        .map(|sf| enum_completions(sf.known_values, prefix, indent))
        .unwrap_or_default()
}

/// Get sub-field name completions for a mapping list field.
pub fn get_subfield_name_completions(
    subfields: &[SubField],
    prefix: &str,
    indent: u32,
) -> Vec<CompletionItem> {
    let normalized = prefix.trim().to_ascii_lowercase();
    let indent_str = make_indent(indent);
    subfields
        .iter()
        .filter(|sf| sf.name.to_ascii_lowercase().starts_with(&normalized))
        .map(|sf| CompletionItem {
            label: sf.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(sf.description.to_string()),
            insert_text: Some(format!("{}: $1\n{indent_str}$0", sf.name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

/// DEP-12 upstream metadata fields.
pub const UPSTREAM_FIELDS: &[UpstreamField] = &[
    UpstreamField::new("Repository", "URL of the upstream source repository"),
    UpstreamField::new(
        "Repository-Browse",
        "Web interface for the upstream repository",
    ),
    UpstreamField::new("Bug-Database", "URL of the upstream bug tracking system"),
    UpstreamField::new("Bug-Submit", "URL for submitting new upstream bugs"),
    UpstreamField::new("Name", "Human-readable name of the upstream project"),
    UpstreamField::new("Contact", "Contact information for the upstream authors"),
    UpstreamField::new("Changelog", "URL of the upstream changelog"),
    UpstreamField::new("Documentation", "URL of the upstream documentation"),
    UpstreamField::new("FAQ", "URL of the upstream FAQ"),
    UpstreamField::new("Donation", "URL for donating to the upstream project"),
    UpstreamField::with_value_type(
        "Screenshots",
        "URL of upstream screenshots",
        FieldValueType::ScalarList,
    ),
    UpstreamField::new("Gallery", "URL of an upstream image gallery"),
    UpstreamField::new("Webservice", "URL of the upstream web service"),
    UpstreamField::new("Security-Contact", "Contact for reporting security issues"),
    UpstreamField::new("CPE", "Common Platform Enumeration identifier"),
    UpstreamField::new("ASCL-Id", "Astrophysics Source Code Library identifier"),
    UpstreamField::new("Cite-As", "Preferred citation for the software"),
    UpstreamField::with_value_type(
        "Funding",
        "Funding information for the project",
        FieldValueType::MappingList(FUNDING_SUBFIELDS),
    ),
    UpstreamField::with_value_type(
        "Reference",
        "Bibliographic references for the software",
        FieldValueType::MappingList(REFERENCE_SUBFIELDS),
    ),
    UpstreamField::with_value_type(
        "Registry",
        "External software registry entries",
        FieldValueType::MappingList(REGISTRY_SUBFIELDS),
    ),
    UpstreamField::with_value_type(
        "Other-References",
        "Additional references not covered by Reference",
        FieldValueType::ScalarList,
    ),
];

/// Look up the standard (canonical) casing for a field name.
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    let lowercase = field_name.to_lowercase();
    UPSTREAM_FIELDS
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| f.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_fields() {
        assert_eq!(UPSTREAM_FIELDS.len(), 21);

        let names: Vec<_> = UPSTREAM_FIELDS.iter().map(|f| f.name).collect();
        assert_eq!(names[0], "Repository");
        assert_eq!(names[1], "Repository-Browse");
        assert_eq!(names[2], "Bug-Database");
        assert_eq!(names[3], "Bug-Submit");
        assert_eq!(names[4], "Name");
    }

    #[test]
    fn test_upstream_field_validity() {
        for field in UPSTREAM_FIELDS {
            assert!(!field.name.is_empty(), "Field name must not be empty");
            assert!(
                !field.description.is_empty(),
                "Description for {} must not be empty",
                field.name
            );
        }
    }

    #[test]
    fn test_get_standard_field_name() {
        assert_eq!(get_standard_field_name("Repository"), Some("Repository"));
        assert_eq!(get_standard_field_name("repository"), Some("Repository"));
        assert_eq!(
            get_standard_field_name("bug-database"),
            Some("Bug-Database")
        );
        assert_eq!(get_standard_field_name("CPE"), Some("CPE"));
        assert_eq!(get_standard_field_name("cpe"), Some("CPE"));
        assert_eq!(get_standard_field_name("UnknownField"), None);
    }
}
