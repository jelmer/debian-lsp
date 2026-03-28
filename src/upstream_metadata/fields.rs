/// The type of value a DEP-12 field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValueType {
    /// A URL (e.g. Repository, Bug-Database).
    Url,
    /// Free-form text or structured non-URL data.
    Text,
}

/// A field definition for the upstream/metadata file (DEP-12).
pub struct UpstreamField {
    pub name: &'static str,
    pub description: &'static str,
    pub value_type: FieldValueType,
}

impl UpstreamField {
    pub const fn new(
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

/// DEP-12 upstream metadata fields.
pub const UPSTREAM_FIELDS: &[UpstreamField] = &[
    UpstreamField::new(
        "Repository",
        "URL of the upstream source repository",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Repository-Browse",
        "Web interface for the upstream repository",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Bug-Database",
        "URL of the upstream bug tracking system",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Bug-Submit",
        "URL for submitting new upstream bugs",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Name",
        "Human-readable name of the upstream project",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Contact",
        "Contact information for the upstream authors",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Changelog",
        "URL of the upstream changelog",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Documentation",
        "URL of the upstream documentation",
        FieldValueType::Url,
    ),
    UpstreamField::new("FAQ", "URL of the upstream FAQ", FieldValueType::Url),
    UpstreamField::new(
        "Donation",
        "URL for donating to the upstream project",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Screenshots",
        "URL of upstream screenshots",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Gallery",
        "URL of an upstream image gallery",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Webservice",
        "URL of the upstream web service",
        FieldValueType::Url,
    ),
    UpstreamField::new(
        "Security-Contact",
        "Contact for reporting security issues",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "CPE",
        "Common Platform Enumeration identifier",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "ASCL-Id",
        "Astrophysics Source Code Library identifier",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Cite-As",
        "Preferred citation for the software",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Funding",
        "Funding information for the project",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Reference",
        "Bibliographic references for the software",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Registry",
        "External software registry entries",
        FieldValueType::Text,
    ),
    UpstreamField::new(
        "Other-References",
        "Additional references not covered by Reference",
        FieldValueType::Text,
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

/// Look up the value type for a field name (case-insensitive).
pub fn get_field_value_type(field_name: &str) -> Option<FieldValueType> {
    let lowercase = field_name.to_lowercase();
    UPSTREAM_FIELDS
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| f.value_type)
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
