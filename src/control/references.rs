//! Find-references for binary package names in debian/control.
//!
//! When the cursor is on a `Package:` field value, finds all locations in the
//! same control file where that package name appears in relationship fields
//! (Depends, Build-Depends, etc.).

use debian_control::lossless::relations::Relations;
use debian_control::lossless::{Control, Parse};
use debian_control::relations::SyntaxKind as RelSyntaxKind;
use tower_lsp_server::ls_types::{Location, Position, Uri};

use super::relation_completion::is_relationship_field;
use crate::position::{text_range_to_lsp_range, try_position_to_offset};

/// Find all locations in a control file where the given package name appears
/// in relationship fields.
fn find_package_references_in_control(
    parse: &Parse<Control>,
    source_text: &str,
    package_name: &str,
    uri: &Uri,
    include_declaration: bool,
) -> Vec<Location> {
    let control = parse.tree();
    let mut locations = Vec::new();

    // Optionally include the Package: field declaration itself.
    if include_declaration {
        for binary in control.binaries() {
            if binary.name().as_deref() == Some(package_name) {
                let para = binary.as_deb822();
                if let Some(entry) = para.get_entry("Package") {
                    if let Some(value_range) = entry.value_range() {
                        let range = text_range_to_lsp_range(source_text, value_range);
                        locations.push(Location {
                            uri: uri.clone(),
                            range,
                        });
                    }
                }
            }
        }
    }

    // Scan all relationship fields for references to the package name.
    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let Some(field_name) = entry.key() else {
                continue;
            };
            if !is_relationship_field(&field_name) {
                continue;
            }

            let Some(value_range) = entry.value_range() else {
                continue;
            };
            let value_start: usize = value_range.start().into();
            let value_end: usize = value_range.end().into();
            let raw_value = &source_text[value_start..value_end];

            let (relations, _errors) = Relations::parse_relaxed(raw_value, false);
            let syntax = relations.syntax();

            let mut tok = syntax.first_token();
            while let Some(t) = tok {
                if t.kind() == RelSyntaxKind::IDENT {
                    if let Some(parent) = t.parent() {
                        if parent.kind() == RelSyntaxKind::RELATION && t.text() == package_name {
                            let rel_start: usize = t.text_range().start().into();
                            let rel_end: usize = t.text_range().end().into();
                            let abs_range = rowan::TextRange::new(
                                ((value_start + rel_start) as u32).into(),
                                ((value_start + rel_end) as u32).into(),
                            );
                            let range = text_range_to_lsp_range(source_text, abs_range);
                            locations.push(Location {
                                uri: uri.clone(),
                                range,
                            });
                        }
                    }
                }
                tok = t.next_token();
            }
        }
    }

    locations
}

/// Find references to a binary package name at the given cursor position.
///
/// The cursor must be on a `Package:` field value or on a package name
/// in a relationship field. Returns all locations in the control file
/// where that package is referenced.
pub fn find_references(
    parse: &Parse<Control>,
    source_text: &str,
    position: Position,
    uri: &Uri,
    include_declaration: bool,
) -> Vec<Location> {
    let control = parse.tree();
    let Some(offset) = try_position_to_offset(source_text, position) else {
        return Vec::new();
    };

    // Check if cursor is on a Package: field value.
    for binary in control.binaries() {
        let para = binary.as_deb822();
        let Some(entry) = para.get_entry("Package") else {
            continue;
        };
        let Some(value_range) = entry.value_range() else {
            continue;
        };
        if value_range.contains(offset) || value_range.end() == offset {
            if let Some(name) = binary.name() {
                return find_package_references_in_control(
                    parse,
                    source_text,
                    &name,
                    uri,
                    include_declaration,
                );
            }
        }
    }

    // Check if cursor is on a package name inside a relationship field.
    let deb822 = control.as_deb822();
    let entry = deb822
        .paragraphs()
        .flat_map(|p| p.entries().collect::<Vec<_>>())
        .find(|entry| {
            let r = entry.text_range();
            r.start() <= offset && offset < r.end()
        });

    let Some(entry) = entry else {
        return Vec::new();
    };

    let Some(field_name) = entry.key() else {
        return Vec::new();
    };

    if !is_relationship_field(&field_name) {
        return Vec::new();
    }

    let Some(value_range) = entry.value_range() else {
        return Vec::new();
    };
    let value_start: usize = value_range.start().into();
    let value_end: usize = value_range.end().into();
    let raw_value = &source_text[value_start..value_end];
    let offset_in_value = usize::from(offset) - value_start;

    // Find the package name at the cursor offset.
    let (relations, _errors) = Relations::parse_relaxed(raw_value, false);
    let syntax = relations.syntax();

    let mut package_name = None;
    let mut tok = syntax.first_token();
    while let Some(t) = tok {
        if t.kind() == RelSyntaxKind::IDENT {
            let start: usize = t.text_range().start().into();
            let end: usize = t.text_range().end().into();
            if start <= offset_in_value && offset_in_value <= end {
                if let Some(parent) = t.parent() {
                    if parent.kind() == RelSyntaxKind::RELATION {
                        package_name = Some(t.text().to_string());
                        break;
                    }
                }
            }
        }
        tok = t.next_token();
    }

    let Some(name) = package_name else {
        return Vec::new();
    };

    // Only find references for packages defined in this control file.
    let is_local = control
        .binaries()
        .any(|b| b.name().as_deref() == Some(&name));
    if !is_local {
        return Vec::new();
    }

    find_package_references_in_control(parse, source_text, &name, uri, include_declaration)
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
    fn test_references_from_package_field() {
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

        // Cursor on "mypackage-dev" in Package: field (line 9)
        let refs = find_references(&parsed, text, Position::new(9, 10), &uri, true);
        // Should find: the Package: declaration, Build-Depends ref, Depends ref
        assert_eq!(refs.len(), 3);
        // Declaration
        assert_eq!(refs[0].range.start.line, 9);
        // Build-Depends
        assert_eq!(refs[1].range.start.line, 2);
        // Depends
        assert_eq!(refs[2].range.start.line, 6);
    }

    #[test]
    fn test_references_from_package_field_no_declaration() {
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

        // Cursor on "mypackage-dev" in Package: field (line 9), no declaration
        let refs = find_references(&parsed, text, Position::new(9, 10), &uri, false);
        // Should find: Build-Depends ref, Depends ref (no declaration)
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].range.start.line, 2);
        assert_eq!(refs[1].range.start.line, 6);
    }

    #[test]
    fn test_references_from_relationship_field() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>
Build-Depends: foo-dev

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

        // Cursor on "foo-dev" in Depends field (line 6)
        let refs = find_references(&parsed, text, Position::new(6, 10), &uri, true);
        // Should find: Package: declaration, Build-Depends ref, Depends ref
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].range.start.line, 9);
        assert_eq!(refs[1].range.start.line, 2);
        assert_eq!(refs[2].range.start.line, 6);
    }

    #[test]
    fn test_references_external_package() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>
Build-Depends: debhelper

Package: foo
Architecture: any
Description: Main
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "debhelper" - not defined in this file
        let refs = find_references(&parsed, text, Position::new(2, 16), &uri, true);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_references_not_on_package_name() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on Architecture value
        let refs = find_references(&parsed, text, Position::new(4, 16), &uri, true);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_references_package_not_referenced() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main

Package: foo-dev
Architecture: any
Description: Dev files
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // Cursor on "foo-dev" Package: field - not referenced anywhere
        let refs = find_references(&parsed, text, Position::new(7, 10), &uri, true);
        // Should find only the declaration itself
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].range.start.line, 7);
    }

    #[test]
    fn test_references_package_not_referenced_no_declaration() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main

Package: foo-dev
Architecture: any
Description: Dev files
";
        let parsed = Control::parse(text);
        let uri = test_uri();

        // No declaration included, and no references
        let refs = find_references(&parsed, text, Position::new(7, 10), &uri, false);
        assert!(refs.is_empty());
    }
}
