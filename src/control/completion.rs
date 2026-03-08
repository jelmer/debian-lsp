use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::field_at_cursor;
use super::detection::is_control_file;
use super::detection::FieldContext;
use super::fields::{
    ARCHITECTURE_VALUES, BUILD_ESSENTIAL_VALUES, COMMON_PACKAGES, CONTROL_FIELDS, ESSENTIAL_VALUES,
    MULTI_ARCH_VALUES, PRIORITY_VALUES, PROTECTED_VALUES, SECTION_VALUES,
};
use crate::workspace::SourceFile;
use crate::workspace::Workspace;

/// Get completion items for a given position in a control file
pub fn get_completions(
    uri: &Uri,
    _position: Position,
    file: SourceFile,
    workspace: &Workspace,
) -> Vec<CompletionItem> {
    if !is_control_file(uri) {
        return Vec::new();
    }

    let field_ctx = field_at_cursor(workspace, file, _position);
    let mut completions = get_value_completions(field_ctx);

    if !completions.is_empty() {
        return completions;
    }
    completions.extend(get_field_completions());
    completions.extend(get_package_completions());

    completions
}

/// Get completion items for a field value if the cursor is on it
pub fn get_value_completions(
    field_ctx: crate::control::detection::FieldContext,
) -> Vec<CompletionItem> {
    match field_ctx {
        FieldContext::Value(key) => match key.as_str() {
            "Priority" => get_priority_completions(),
            "Section" => get_section_completions(),
            "Protected" => get_protected_completions(),
            "Essential" => get_essential_completions(),
            "Build-Essential" => get_build_essential_completions(),
            "Architecture" => get_architecture_completions(),
            "Multi-Arch" => get_multi_arch_completions(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
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

/// Get completion items for Section values
pub fn get_section_completions() -> Vec<CompletionItem> {
    SECTION_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Section value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for Priority values
pub fn get_priority_completions() -> Vec<CompletionItem> {
    PRIORITY_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Priority value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for Protected values
pub fn get_protected_completions() -> Vec<CompletionItem> {
    PROTECTED_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Protected value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for Essential values
pub fn get_essential_completions() -> Vec<CompletionItem> {
    ESSENTIAL_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Essential value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for build essential values
pub fn get_build_essential_completions() -> Vec<CompletionItem> {
    BUILD_ESSENTIAL_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Build essenetial value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for architecture values
pub fn get_architecture_completions() -> Vec<CompletionItem> {
    ARCHITECTURE_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Architecture value".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for multi arch values
pub fn get_multi_arch_completions() -> Vec<CompletionItem> {
    MULTI_ARCH_VALUES
        .iter()
        .map(|&val| CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Multi arch value".to_string()),
            ..Default::default()
        })
        .collect()
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_get_completions_for_control_file() {
//         let uri = str::parse("file:///path/to/debian/control").unwrap();

//         let position = Position::new(0, 0);

//         let completions = get_completions(&uri, position);
//         assert!(!completions.is_empty());

//         // Should have both field and package completions
//         let field_count = completions
//             .iter()
//             .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
//             .count();
//         let package_count = completions
//             .iter()
//             .filter(|c| c.kind == Some(CompletionItemKind::VALUE))
//             .count();

//         assert!(field_count > 0);
//         assert!(package_count > 0);
//     }

//     #[test]
//     fn test_get_completions_for_non_control_file() {
//         let uri = str::parse("file:///path/to/other.txt").unwrap();
//         let position = Position::new(0, 0);

//         let completions = get_completions(&uri, position);
//         assert!(completions.is_empty());
//     }

//     #[test]
//     fn test_field_completions() {
//         let completions = get_field_completions();

//         assert!(!completions.is_empty());

//         // Check that all completions have required properties
//         for completion in &completions {
//             assert!(!completion.label.is_empty());
//             assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
//             assert!(completion.detail.is_some());
//             assert!(completion.documentation.is_some());
//             assert!(completion.insert_text.is_some());
//             assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
//         }

//         // Check for specific fields
//         let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
//         assert!(labels.iter().any(|l| *l == "Source"));
//         assert!(labels.iter().any(|l| *l == "Package"));
//         assert!(labels.iter().any(|l| *l == "Depends"));
//     }

//     #[test]
//     fn test_package_completions() {
//         let completions = get_package_completions();

//         assert!(!completions.is_empty());

//         // Check that all completions have required properties
//         for completion in &completions {
//             assert!(!completion.label.is_empty());
//             assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
//             assert_eq!(completion.detail, Some("Package name".to_string()));
//         }

//         // Check for specific packages
//         let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
//         assert!(labels.iter().any(|l| *l == "debhelper-compat"));
//         assert!(labels.iter().any(|l| *l == "cmake"));
//     }
// }
