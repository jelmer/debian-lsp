use tower_lsp_server::ls_types::Uri;

pub enum FieldContext {
    Key(String),
    Value(String),
    None,
}

use crate::position::position_to_offset;
use crate::workspace::Workspace;
use text_size::TextRange;
use text_size::TextSize;
use tower_lsp_server::ls_types::Position;

pub fn field_at_cursor(
    workspace: &Workspace,
    file: crate::workspace::SourceFile,
    position: Position,
) -> FieldContext {
    let control_tree = workspace.get_parsed_control(file).tree();
    let text = workspace.source_text(file);
    let offset = position_to_offset(&text, position);
    let range_end = offset + TextSize::from(1);
    let query_range = TextRange::new(offset, range_end);

    if let Some(entry) = control_tree.fields_in_range(query_range).next() {
        if let Some(key) = entry.key() {
            if let Some(key_range) = entry.key_range() {
                if offset >= key_range.end() {
                    return FieldContext::Value(key);
                }
            }
            return FieldContext::Key(key);
        }
    }
    FieldContext::None
}
/// Check if a given URL represents a Debian control file
pub fn is_control_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/control") || path.ends_with("/debian/control")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_control_file() {
        let control_paths = vec![
            "file:///path/to/debian/control",
            "file:///project/debian/control",
            "file:///control",
            "file:///some/path/control",
        ];

        let non_control_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/control.txt",
            "file:///path/to/mycontrol",
            "file:///path/to/debian/control.backup",
        ];

        for path in control_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_control_file(&uri),
                "Should detect control file: {}",
                path
            );
        }

        for path in non_control_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_control_file(&uri),
                "Should not detect as control file: {}",
                path
            );
        }
    }
}
