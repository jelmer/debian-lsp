use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

use super::fields::{
    COMMON_PACKAGES, CONTROL_FIELDS, CONTROL_PRIORITY_VALUES, CONTROL_SECTION_AREAS,
    CONTROL_SECTION_VALUES, CONTROL_SPECIAL_SECTION_VALUES,
};

/// Get completions for a control file at the given cursor position.
///
/// Uses the parsed deb822 document for position-aware completions:
/// if on a field value, returns value completions; otherwise returns
/// field name and package name completions.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: tower_lsp_server::ls_types::Position,
) -> Vec<CompletionItem> {
    let mut completions = crate::deb822::completion::get_completions(
        deb822,
        source_text,
        position,
        CONTROL_FIELDS,
        get_field_value_completions,
    );
    // When returning field completions (not value completions), also
    // include common package names.
    if completions
        .iter()
        .any(|c| c.kind == Some(CompletionItemKind::FIELD))
    {
        completions.extend(get_package_completions());
    }
    completions
}

/// Get value completions for specific control file fields.
pub fn get_field_value_completions(field_name: &str, prefix: &str) -> Vec<CompletionItem> {
    if field_name.eq_ignore_ascii_case("Section") {
        get_section_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Priority") {
        get_priority_value_completions(prefix)
    } else {
        vec![]
    }
}

/// Get completion items for common package names
pub fn get_package_completions() -> Vec<CompletionItem> {
    COMMON_PACKAGES
        .iter()
        .map(|&package| CompletionItem {
            label: package.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Package name".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for Debian priority values.
pub fn get_priority_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    CONTROL_PRIORITY_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(&normalized_prefix))
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for Debian section values.
///
/// Includes both `section` and `area/section` forms.
pub fn get_section_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let mut completions = Vec::new();

    for &(section, description) in CONTROL_SECTION_VALUES {
        if section.starts_with(&normalized_prefix) {
            completions.push(CompletionItem {
                label: section.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(description.to_string()),
                insert_text: Some(section.to_string()),
                ..Default::default()
            });
        }
    }

    for &area in CONTROL_SECTION_AREAS {
        for &(section, description) in CONTROL_SECTION_VALUES {
            let qualified = format!("{}/{}", area, section);
            if qualified.starts_with(&normalized_prefix) {
                completions.push(CompletionItem {
                    label: qualified.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some(description.to_string()),
                    insert_text: Some(qualified),
                    ..Default::default()
                });
            }
        }
    }

    for &(special, description) in CONTROL_SPECIAL_SECTION_VALUES {
        if special.starts_with(&normalized_prefix) {
            completions.push(CompletionItem {
                label: special.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(description.to_string()),
                insert_text: Some(special.to_string()),
                ..Default::default()
            });
        }
    }

    completions
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::Position;

    #[test]
    fn test_get_completions_on_field_key() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 3));

        // Should have both field and package completions
        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        let package_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::VALUE))
            .count();

        assert!(field_count > 0);
        assert!(package_count > 0);
    }

    #[test]
    fn test_get_completions_on_section_value() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 11));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"python"));
    }

    #[test]
    fn test_get_completions_on_priority_value() {
        let text = "Source: test\nPriority: op\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 12));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_package_completions() {
        let completions = get_package_completions();

        assert!(!completions.is_empty());

        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert_eq!(completion.detail, Some("Package name".to_string()));
        }

        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "debhelper-compat"));
        assert!(labels.iter().any(|l| *l == "cmake"));
    }

    #[test]
    fn test_priority_value_completions() {
        let completions = get_priority_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"required"));
        assert!(labels.contains(&"important"));
        assert!(labels.contains(&"standard"));
        assert!(labels.contains(&"optional"));
        assert!(labels.contains(&"extra"));
    }

    #[test]
    fn test_priority_value_completions_with_prefix() {
        let completions = get_priority_value_completions("op");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_priority_value_completions_with_uppercase_prefix() {
        let completions = get_priority_value_completions("OP");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_section_value_completions() {
        let completions = get_section_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"admin"));
        assert!(labels.contains(&"python"));
        assert!(labels.contains(&"debian-installer"));
        assert!(labels.contains(&"non-free/python"));
        assert!(!labels.contains(&"non-free/debian-installer"));

        // Check that descriptions are present
        let admin = completions.iter().find(|c| c.label == "admin").unwrap();
        assert_eq!(
            admin.detail.as_deref(),
            Some("System administration utilities")
        );
    }

    #[test]
    fn test_section_value_completions_with_area_prefix() {
        let completions = get_section_value_completions("non-free/");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"non-free/python"));
        assert!(!labels.contains(&"python"));
        assert!(!labels.contains(&"non-free/debian-installer"));

        // Area-qualified sections use the same description as the base section
        let nf_python = completions
            .iter()
            .find(|c| c.label == "non-free/python")
            .unwrap();
        assert_eq!(
            nf_python.detail.as_deref(),
            Some("Python programming language")
        );
    }

    #[test]
    fn test_get_field_value_completions_for_section() {
        let completions = get_field_value_completions("Section", "py");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"python"));
    }

    #[test]
    fn test_get_field_value_completions_for_priority() {
        let completions = get_field_value_completions("Priority", "op");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_get_field_value_completions_for_unknown_field() {
        let completions = get_field_value_completions("Depends", "py");
        assert!(completions.is_empty());
    }
}
