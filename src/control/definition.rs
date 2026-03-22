//! Go-to-definition for package names in debian/control relationship fields.

use debian_control::lossless::relations::Relations;
use debian_control::lossless::{Control, Parse};
use debian_control::relations::SyntaxKind as RelSyntaxKind;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Location, Position, Uri};

use super::relation_completion::is_relationship_field;
use crate::deb822::completion::{get_cursor_context, CursorContext};
use crate::position::{text_range_to_lsp_range, try_position_to_offset};

/// Find the package name at the cursor position within a relationship field value.
///
/// Parses the full field value, then walks the CST to find the RELATION node
/// whose IDENT token covers the cursor byte offset.
fn find_package_at_offset(value: &str, offset_in_value: usize) -> Option<String> {
    let (relations, _errors) = Relations::parse_relaxed(value, false);
    let syntax = relations.syntax();

    // Walk all tokens to find the IDENT inside a RELATION node that covers the offset.
    let mut tok = syntax.first_token();
    while let Some(t) = tok {
        if t.kind() == RelSyntaxKind::IDENT {
            let range = t.text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            if start <= offset_in_value && offset_in_value <= end {
                // Check this IDENT is a package name (direct child of RELATION node)
                if let Some(parent) = t.parent() {
                    if parent.kind() == RelSyntaxKind::RELATION {
                        return Some(t.text().to_string());
                    }
                }
            }
        }
        tok = t.next_token();
    }
    None
}

/// Try to resolve go-to-definition for a package name in a relationship field.
///
/// Returns a `Location` pointing to the binary package paragraph in the same
/// control file, if the cursor is on a package name that matches one of the
/// binary packages defined in the file.
pub fn goto_definition(
    parse: &Parse<Control>,
    source_text: &str,
    position: Position,
    uri: &Uri,
) -> Option<Location> {
    let control = parse.tree();
    let deb822 = control.as_deb822();

    // Determine cursor context — we only handle relationship field values.
    let ctx = get_cursor_context(deb822, source_text, position)?;
    let (field_name, _value_prefix) = match ctx {
        CursorContext::FieldValue {
            field_name,
            value_prefix,
        } => (field_name, value_prefix),
        _ => return None,
    };

    if !is_relationship_field(&field_name) {
        return None;
    }

    // Find the entry that contains the cursor to get the full value and its byte range.
    let offset = try_position_to_offset(source_text, position)?;
    let entry = deb822
        .paragraphs()
        .flat_map(|p| p.entries().collect::<Vec<_>>())
        .find(|entry| {
            let r = entry.text_range();
            r.start() <= offset && offset < r.end()
        })?;

    let value_range = entry.value_range()?;
    let value_start: usize = value_range.start().into();
    let offset_usize: usize = offset.into();

    // Get the raw value text from the source, stripping continuation-line
    // leading whitespace the same way the completion code does.
    let value_end: usize = value_range.end().into();
    let raw_value = &source_text[value_start..value_end];

    // The value_prefix gives us text up to cursor with continuation indentation
    // stripped. But for CST offset computation we need the offset within the
    // raw value text (which includes continuation indentation).
    let offset_in_value = offset_usize - value_start;

    let package_name = find_package_at_offset(raw_value, offset_in_value)?;

    // Look for a matching binary package paragraph.
    for binary in control.binaries() {
        if binary.name().as_deref() == Some(package_name.as_str()) {
            let para = binary.as_deb822();
            let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
            return Some(Location {
                uri: uri.clone(),
                range,
            });
        }
    }

    // Also check the source paragraph name.
    if let Some(source) = control.source() {
        if source.name().as_deref() == Some(package_name.as_str()) {
            let para = source.as_deb822();
            let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
            return Some(Location {
                uri: uri.clone(),
                range,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri() -> Uri {
        if cfg!(windows) {
            Uri::from_file_path("C:\\tmp\\debian\\control").unwrap()
        } else {
            Uri::from_file_path("/tmp/debian/control").unwrap()
        }
    }

    #[test]
    fn test_goto_definition_binary_package() {
        let text = "\
Source: mypackage
Maintainer: Test <test@example.com>
Build-Depends: debhelper, mypackage-dev

Package: mypackage
Architecture: any
Depends: mypackage-dev
Description: Main package

Package: mypackage-dev
Architecture: any
Description: Development files
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "mypackage-dev" in Build-Depends (line 2, on the 'm' of mypackage-dev)
        let result = goto_definition(&parsed, text, Position::new(2, 26), &uri);
        assert!(result.is_some(), "Should find definition for mypackage-dev");
        let loc = result.unwrap();
        // Should point to the "Package: mypackage-dev" paragraph
        assert_eq!(loc.range.start.line, 9);
    }

    #[test]
    fn test_goto_definition_in_depends() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Depends: foo-dev
Description: Main

Package: foo-dev
Architecture: any
Description: Dev files
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "foo-dev" in Depends field (line 5, character 10)
        let result = goto_definition(&parsed, text, Position::new(5, 10), &uri);
        assert!(result.is_some(), "Should find definition for foo-dev");
        let loc = result.unwrap();
        assert_eq!(loc.range.start.line, 8);
    }

    #[test]
    fn test_goto_definition_not_on_relationship_field() {
        let text = "\
Source: mypackage
Maintainer: Test <test@example.com>

Package: mypackage
Architecture: any
Description: A package
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "any" in Architecture field — not a relationship field
        let result = goto_definition(&parsed, text, Position::new(4, 16), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_external_package() {
        let text = "\
Source: mypackage
Maintainer: Test <test@example.com>
Build-Depends: debhelper

Package: mypackage
Architecture: any
Description: A package
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "debhelper" — not defined in this control file
        let result = goto_definition(&parsed, text, Position::new(2, 18), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_multiline_depends() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>
Build-Depends:
 debhelper-compat (= 13),
 foo-dev

Package: foo
Architecture: any
Description: Main

Package: foo-dev
Architecture: any
Description: Dev
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "foo-dev" in multiline Build-Depends (line 4, character 1)
        let result = goto_definition(&parsed, text, Position::new(4, 2), &uri);
        assert!(
            result.is_some(),
            "Should find definition for foo-dev in multiline field"
        );
        let loc = result.unwrap();
        assert_eq!(loc.range.start.line, 10);
    }

    #[test]
    fn test_find_package_at_offset_simple() {
        let name = find_package_at_offset("debhelper, foo-dev", 12);
        assert_eq!(name.as_deref(), Some("foo-dev"));
    }

    #[test]
    fn test_find_package_at_offset_first() {
        let name = find_package_at_offset("debhelper, foo-dev", 3);
        assert_eq!(name.as_deref(), Some("debhelper"));
    }

    #[test]
    fn test_find_package_at_offset_with_version() {
        let name = find_package_at_offset("debhelper-compat (= 13), foo-dev", 26);
        assert_eq!(name.as_deref(), Some("foo-dev"));
    }
}
