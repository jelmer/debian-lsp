use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_control_file;
use super::fields::{
    COMMON_PACKAGES, CONTROL_FIELDS, CONTROL_PRIORITY_VALUES, CONTROL_SECTION_AREAS,
    CONTROL_SECTION_VALUES,
};

/// Get completion items for a given position in a control file
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_control_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_field_completions());
    completions.extend(get_package_completions());
    completions
}

/// Get completion items for a given position in a control file with source context.
///
/// This enables field-value completions for `Section` and `Priority`.
pub fn get_completions_with_source(
    uri: &Uri,
    position: Position,
    source_text: &str,
) -> Vec<CompletionItem> {
    if !is_control_file(uri) {
        return Vec::new();
    }

    if let Some((field_name, prefix)) = detect_field_value_context(source_text, position) {
        if field_name.eq_ignore_ascii_case("Section") {
            return get_section_value_completions(prefix);
        }
        if field_name.eq_ignore_ascii_case("Priority") {
            return get_priority_value_completions(prefix);
        }
    }

    get_completions(uri, position)
}

/// Get completion items for control file fields
pub fn get_field_completions() -> Vec<CompletionItem> {
    CONTROL_FIELDS
        .iter()
        .map(|field| CompletionItem {
            label: field.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(field.description.to_string()),
            documentation: Some(Documentation::String(field.description.to_string())),
            insert_text: Some(format!("{}: ", field.name)),
            ..Default::default()
        })
        .collect()
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
        .filter(|value| value.starts_with(&normalized_prefix))
        .map(|&value| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Priority value".to_string()),
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

    for &section in CONTROL_SECTION_VALUES {
        if section.starts_with(&normalized_prefix) {
            completions.push(CompletionItem {
                label: section.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some("Section value".to_string()),
                insert_text: Some(section.to_string()),
                ..Default::default()
            });
        }
    }

    for &area in CONTROL_SECTION_AREAS {
        for &section in CONTROL_SECTION_VALUES {
            let qualified = format!("{}/{}", area, section);
            if qualified.starts_with(&normalized_prefix) {
                completions.push(CompletionItem {
                    label: qualified.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some("Section value (area/section)".to_string()),
                    insert_text: Some(qualified),
                    ..Default::default()
                });
            }
        }
    }

    completions
}

/// Detect whether the cursor is in a field value position on the current line.
///
/// Returns (field_name, value_prefix_before_cursor) when the cursor is after
/// the `:` delimiter on the same line.
fn detect_field_value_context(source_text: &str, position: Position) -> Option<(&str, &str)> {
    let line = source_text.lines().nth(position.line as usize)?;
    let cursor_byte = char_to_byte_offset(line, position.character);
    let line_before_cursor = &line[..cursor_byte];
    let colon_index = line_before_cursor.find(':')?;

    // If cursor is at the colon itself, we are not in value context yet.
    if cursor_byte <= colon_index + 1 {
        return None;
    }

    let field_name = line_before_cursor[..colon_index].trim();
    if field_name.is_empty() {
        return None;
    }

    let value_part = &line_before_cursor[colon_index + 1..];
    let value_prefix = value_part.trim_start();
    Some((field_name, value_prefix))
}

/// Convert a line character offset to a byte offset, clamped to line length.
fn char_to_byte_offset(line: &str, character: u32) -> usize {
    let character = character as usize;
    line.char_indices()
        .nth(character)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(line.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_for_control_file() {
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(!completions.is_empty());

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
    fn test_get_completions_for_non_control_file() {
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_field_completions() {
        let completions = get_field_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }

        // Check for specific fields
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "Source"));
        assert!(labels.iter().any(|l| *l == "Package"));
        assert!(labels.iter().any(|l| *l == "Depends"));
    }

    #[test]
    fn test_package_completions() {
        let completions = get_package_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert_eq!(completion.detail, Some("Package name".to_string()));
        }

        // Check for specific packages
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
    }

    #[test]
    fn test_section_value_completions_with_area_prefix() {
        let completions = get_section_value_completions("non-free/");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"non-free/python"));
        assert!(!labels.contains(&"python"));
    }

    #[test]
    fn test_get_completions_with_source_for_section_value() {
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let source = "Source: foo\nSection: py\n";
        let position = Position::new(1, 11);

        let completions = get_completions_with_source(&uri, position, source);
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"python"));
        assert!(!labels.contains(&"Source"));
    }

    #[test]
    fn test_get_completions_with_source_for_priority_value() {
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let source = "Source: foo\nPriority: op\n";
        let position = Position::new(1, 12);

        let completions = get_completions_with_source(&uri, position, source);
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_get_completions_with_source_fallback_to_default() {
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let source = "Source: foo\nMaintainer: test@example.com\n";
        let position = Position::new(1, 3);

        let completions = get_completions_with_source(&uri, position, source);

        assert!(completions.iter().any(|c| c.label == "Source"));
        assert!(completions.iter().any(|c| c.label == "debhelper-compat"));
    }

    #[test]
    fn test_detect_field_value_context() {
        let source = "Priority: optional\n";
        let position = Position::new(0, 17);

        let context = detect_field_value_context(source, position);
        assert_eq!(context, Some(("Priority", "optiona")));
    }
}
