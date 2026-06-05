//! Go-to-definition for package names in debian/tests/control.

use debian_control::lossless::relations::Relations;
use debian_control::lossless::{Control, Parse};
use tower_lsp_server::ls_types::{Location, Position, Range, Uri};

use crate::deb822::completion::{get_cursor_context, CursorContext};
use crate::position::{LineIndex, Source};

const DEFAULT_TESTS_DIRECTORY: &str = "debian/tests";

/// Find the test name within a `Tests:` field value.
///
/// Scans left to find the start of the token and right to find its end,
/// so the full name is returned even when the cursor is in the middle of it.
/// Returns `None` when the cursor sits on whitespace between tokens.
fn find_test_name_at_offset(value: &str, offset_in_value: usize) -> Option<String> {
    let ch = value[offset_in_value..].chars().next().unwrap_or(' ');
    if ch == ' ' || ch == '\n' {
        return None;
    }

    let start = value[..offset_in_value]
        .rfind(|c: char| c == ' ' || c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    let end = value[offset_in_value..]
        .find(|c: char| c == ' ' || c == '\n')
        .map(|i| i + offset_in_value)
        .unwrap_or(value.len());

    let token = value[start..end].trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Find the package name at the cursor position within a relationship field value.
///
/// Parses the full field value, then walks the CST to find the RELATION node
/// whose IDENT token covers the cursor byte offset.
fn find_package_at_offset(value: &str, offset_in_value: usize) -> Option<String> {
    let (relations, _errors) = Relations::parse_relaxed(value, false);
    for entry in relations.entries() {
        for relation in entry.relations() {
            let range = relation.syntax().text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            if start <= offset_in_value && offset_in_value <= end {
                return relation.try_name();
            }
        }
    }
    None
}

/// Try to resolve go-to-definition for a cursor position in `debian/tests/control`.
///
/// Returns a `Location` pointing to the test script on disk if the cursor is on
/// a name listed in a `Tests:` field and that script exists in the tests directory
/// (Tests-Directory if set, otherwise debian/tests/).  For relationship fields
/// such as `Depends:`, returns a `Location` pointing to the matching binary
/// package paragraph in the project's `debian/control`.
pub fn goto_definition(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
    uri: &Uri,
) -> Option<Location> {
    // Determine cursor context.
    let ctx = get_cursor_context(deb822, src, position)?;
    let (field_name, _value_prefix) = match ctx {
        CursorContext::FieldValue {
            field_name,
            value_prefix,
        } => (field_name, value_prefix),
        _ => return None,
    };

    // Find the entry that contains the cursor to get the full value and its byte range.
    let offset = src.try_position_to_offset(position)?;
    let paragraph = deb822.paragraph_at_position(offset)?;
    let entry = paragraph.entry_at_position(offset)?;

    let value_range = entry.value_range()?;
    let value_start: usize = value_range.start().into();
    let value_end: usize = value_range.end().into();
    let raw_value = &src.text[value_start..value_end];
    let offset_in_value = usize::from(offset) - value_start;

    if field_name.eq_ignore_ascii_case("Tests") {
        goto_definition_tests(deb822, offset, raw_value, offset_in_value, uri)
    } else if field_name.eq_ignore_ascii_case("Tests-Directory") {
        goto_definition_tests_directory(raw_value, offset_in_value, uri)
    } else if crate::control::relation_completion::is_relationship_field(&field_name) {
        goto_definition_in_control(raw_value, offset_in_value, uri)
    } else {
        None
    }
}

/// Resolve a test name in a `Tests:` field to its script on disk.
fn goto_definition_tests(
    deb822: &deb822_lossless::Deb822,
    offset: text_size::TextSize,
    raw_value: &str,
    offset_in_value: usize,
    uri: &Uri,
) -> Option<Location> {
    let test_name = find_test_name_at_offset(raw_value, offset_in_value)?;
    let current_paragraph = deb822.paragraph_at_position(offset)?;
    let root = source_root(uri)?;

    let tests_dir = current_paragraph
        .get("Tests-Directory")
        .map(|v| root.join(v.trim()))
        .unwrap_or_else(|| root.join(DEFAULT_TESTS_DIRECTORY));

    let test_path = tests_dir.join(&test_name);
    if !test_path.is_file() {
        return None;
    }

    Some(Location {
        uri: Uri::from_file_path(&test_path)?,
        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
    })
}

/// Resolve a package name in a relationship field to a binary package paragraph
/// in the project's `debian/control`.
///
/// Reads and parses `debian/control` at call time, then looks for a matching
/// binary package paragraph, mirroring what `control::definition` does for
/// intra-file jumps.
fn goto_definition_in_control(
    raw_value: &str,
    offset_in_value: usize,
    tests_control_uri: &Uri,
) -> Option<Location> {
    let package_name = find_package_at_offset(raw_value, offset_in_value)?;

    // debian/tests/control -> debian/tests -> debian/control
    let control_path = tests_control_uri
        .to_file_path()?
        .parent()?
        .parent()?
        .join("control");
    let control_uri = Uri::from_file_path(&control_path)?;
    let control_text = std::fs::read_to_string(&control_path).ok()?;

    let parsed: Parse<Control> = Control::parse(&control_text);
    let control = parsed.tree();
    let idx = LineIndex::new(&control_text);
    let control_src = Source::new(&control_text, &idx);

    // Look for a matching binary package paragraph.
    crate::control::definition::find_binary_package_location(
        &control,
        control_src,
        &control_uri,
        &package_name,
    )
}

/// Try to resolve go-to-definition for a directory path in a `Tests-Directory:` field.
///
/// Returns a `Location` pointing to the directory on disk, if the cursor is on
/// a path that resolves to an existing directory relative to the source root.
fn goto_definition_tests_directory(
    raw_value: &str,
    offset_in_value: usize,
    uri: &Uri,
) -> Option<Location> {
    let dir_name = find_test_name_at_offset(raw_value, offset_in_value)?;
    let dir_path = source_root(uri)?.join(dir_name.trim());

    if !dir_path.is_dir() {
        return None;
    }

    Some(Location {
        uri: Uri::from_file_path(&dir_path)?,
        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
    })
}

/// Derive the source root from a `debian/tests/control` URI.
///
/// debian/tests/control -> debian/tests -> debian -> source root
fn source_root(uri: &Uri) -> Option<std::path::PathBuf> {
    let control_path = uri.to_file_path()?;
    Some(control_path.parent()?.parent()?.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tests_control(dir: &std::path::Path, content: &str) -> Uri {
        let tests_dir = dir.join("debian").join("tests");
        std::fs::create_dir_all(&tests_dir).unwrap();
        let control = tests_dir.join("control");
        std::fs::write(&control, content).unwrap();
        Uri::from_file_path(&control).unwrap()
    }

    fn write_debian_control(dir: &std::path::Path, content: &str) {
        let debian_dir = dir.join("debian");
        std::fs::create_dir_all(&debian_dir).unwrap();
        std::fs::write(debian_dir.join("control"), content).unwrap();
    }

    #[test]
    fn test_goto_existing_test() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "smoke" in Tests field (line 0, col 9)
        let result = goto_definition(&deb822, src, Position::new(0, 9), &uri);
        assert!(result.is_some(), "Should find the smoke test");
        assert_eq!(
            result.unwrap().uri,
            Uri::from_file_path(dir.path().join("debian/tests/smoke")).unwrap()
        );
    }

    #[test]
    fn test_goto_cursor_in_middle_of_test_name() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "mo" in "smoke" — should still resolve the full name.
        let result = goto_definition(&deb822, src, Position::new(0, 9), &uri);
        assert!(
            result.is_some(),
            "Cursor mid-token should resolve full name"
        );
    }

    #[test]
    fn test_goto_second_test_in_list() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke integration\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/integration"), "#!/bin/sh\n").unwrap();

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor somewhere inside "integration" (line 0, col 16)
        let result = goto_definition(&deb822, src, Position::new(0, 16), &uri);
        assert!(result.is_some(), "Should find the integration test");
        assert_eq!(
            result.unwrap().uri,
            Uri::from_file_path(dir.path().join("debian/tests/integration")).unwrap()
        );
    }

    #[test]
    fn test_goto_respects_tests_directory() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nTests-Directory: t\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        let custom = dir.path().join("t");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("smoke"), "#!/bin/sh\n").unwrap();

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(&deb822, src, Position::new(0, 9), &uri);
        assert!(result.is_some(), "Should resolve via Tests-Directory");
        assert_eq!(
            result.unwrap().uri,
            Uri::from_file_path(custom.join("smoke")).unwrap()
        );
    }

    #[test]
    fn test_goto_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: ghost\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "ghost" — script does not exist on disk
        let result = goto_definition(&deb822, src, Position::new(0, 9), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_not_on_relationship_field() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nRestrictions: needs-root\n";
        let uri = write_tests_control(dir.path(), content);

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "needs-root" in Restrictions field — not a handled field
        let result = goto_definition(&deb822, src, Position::new(1, 16), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_cursor_on_separator_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke integration\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on the space between "smoke" and "integration" (col 13)
        let result = goto_definition(&deb822, src, Position::new(0, 13), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_in_depends() {
        let dir = tempfile::tempdir().unwrap();
        let tests_content = "Tests: smoke\nDepends: mypkg\n";
        let uri = write_tests_control(dir.path(), tests_content);
        write_debian_control(
            dir.path(),
            "\
Source: mypkg
Maintainer: Test <test@example.com>

Package: mypkg
Architecture: any
Description: My package
",
        );

        let deb822 = deb822_lossless::Deb822::parse(tests_content)
            .to_result()
            .unwrap();
        let idx = crate::position::LineIndex::new(tests_content);
        let src = Source::new(tests_content, &idx);

        // Cursor on "mypkg" in Depends (line 1, col 10)
        let result = goto_definition(&deb822, src, Position::new(1, 10), &uri);
        assert!(result.is_some(), "Should find definition for mypkg");
        let loc = result.unwrap();
        assert_eq!(
            loc.uri,
            Uri::from_file_path(dir.path().join("debian").join("control")).unwrap()
        );
        // "Package: mypkg" paragraph starts at line 3
        assert_eq!(loc.range.start.line, 3);
    }

    #[test]
    fn test_goto_definition_external_package() {
        let dir = tempfile::tempdir().unwrap();
        let tests_content = "Tests: smoke\nDepends: debhelper\n";
        let uri = write_tests_control(dir.path(), tests_content);
        write_debian_control(
            dir.path(),
            "\
Source: mypkg
Maintainer: Test <test@example.com>

Package: mypkg
Architecture: any
Description: My package
",
        );

        let deb822 = deb822_lossless::Deb822::parse(tests_content)
            .to_result()
            .unwrap();
        let idx = crate::position::LineIndex::new(tests_content);
        let src = Source::new(tests_content, &idx);

        // Cursor on "debhelper" — not defined in debian/control
        let result = goto_definition(&deb822, src, Position::new(1, 10), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_tests_directory() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nTests-Directory: t\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        let custom = dir.path().join("t");
        std::fs::create_dir_all(&custom).unwrap();

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "t" in Tests-Directory (line 1, col 17)
        let result = goto_definition(&deb822, src, Position::new(1, 17), &uri);
        assert!(
            result.is_some(),
            "Should fincrate d definition for Tests-Directory"
        );
        assert_eq!(result.unwrap().uri, Uri::from_file_path(&custom).unwrap());
    }

    #[test]
    fn test_goto_definition_tests_directory_missing() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nTests-Directory: ghost\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);

        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = crate::position::LineIndex::new(content);
        let src = Source::new(content, &idx);

        // Cursor on "ghost" in Tests-Directory — directory does not exist
        let result = goto_definition(&deb822, src, Position::new(1, 17), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_test_name_at_offset_simple() {
        let name = find_test_name_at_offset("smoke integration", 8);
        assert_eq!(name.as_deref(), Some("integration"));
    }

    #[test]
    fn test_find_test_name_at_offset_first() {
        let name = find_test_name_at_offset("smoke integration", 2);
        assert_eq!(name.as_deref(), Some("smoke"));
    }

    #[test]
    fn test_find_test_name_at_offset_mid_token_resolves_full() {
        // cursor at offset 2 inside "file1" must return the full token, not "fil"
        let name = find_test_name_at_offset("file1 file2", 2);
        assert_eq!(name.as_deref(), Some("file1"));
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
