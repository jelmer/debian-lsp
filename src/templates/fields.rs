use crate::deb822::completion::FieldInfo;

/// Canonical debconf template field names with one-line descriptions.
pub const TEMPLATES_FIELDS: &[FieldInfo] = &[
    FieldInfo::new("Template", "Template name (slash-separated). Required."),
    FieldInfo::new("Type", "Widget type of the question. Required."),
    FieldInfo::new(
        "Description",
        "Short description, then an optional indented body. Required.",
    ),
    FieldInfo::new("_Description", "Translatable `Description` (po-debconf)."),
    FieldInfo::new(
        "Choices",
        "Comma-separated options for `select`/`multiselect`.",
    ),
    FieldInfo::new("_Choices", "Translatable `Choices` (po-debconf)."),
    FieldInfo::new(
        "__Choices",
        "Translatable `Choices`, split per item (po-debconf).",
    ),
    FieldInfo::new("Default", "Default answer."),
    FieldInfo::new("_Default", "Translatable `Default` (po-debconf)."),
];

/// Debconf template `Type` values with a one-line description each.
pub const TEMPLATE_TYPES: &[(&str, &str)] = &[
    ("string", "Free-form single-line text input."),
    ("password", "Free-form input that is not echoed back."),
    (
        "boolean",
        "A yes/no question; the answer is `true` or `false`.",
    ),
    ("select", "Pick a single option from `Choices`."),
    ("multiselect", "Pick zero or more options from `Choices`."),
    ("note", "An informative note shown to the user (no input)."),
    ("error", "A note flagging an error condition (no input)."),
    ("text", "A block of text shown to the user (no input)."),
    ("title", "Sets the title of the following questions."),
];

/// Look up the canonical casing for a template field name.
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    crate::deb822::completion::get_standard_field_name(TEMPLATES_FIELDS, field_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_casing_canonicalises_lowercase() {
        assert_eq!(get_standard_field_name("template"), Some("Template"));
        assert_eq!(
            get_standard_field_name("_description"),
            Some("_Description")
        );
        assert_eq!(get_standard_field_name("Type"), Some("Type"));
    }

    #[test]
    fn standard_casing_returns_none_for_unknown() {
        assert_eq!(get_standard_field_name("Description-fr.UTF-8"), None);
        assert_eq!(get_standard_field_name("X-Custom"), None);
    }
}
