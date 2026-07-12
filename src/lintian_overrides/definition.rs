//! Go-to-definition for package names in lintian-overrides files.

use tower_lsp_server::ls_types::{Location, Position, Uri};

use crate::position::Source;
use lintian_overrides::LintianOverrides;

/// Try to resolve go-to-definition for a cursor position in a lintian-overrides file.
///
/// Only handles package names in the package spec (the part before `:`).
/// Returns a `Location` pointing to the matching binary package paragraph
/// in the project's `debian/control`.
pub fn goto_definition(
    overrides: &LintianOverrides,
    src: Source<'_>,
    position: Position,
    control: &debian_control::lossless::Control,
    control_src: Source<'_>,
    control_uri: &Uri,
) -> Option<Location> {
    let offset = src.try_position_to_offset(position)?;

    let package_name = overrides.package_name_at_offset(offset)?;

    crate::control::definition::find_binary_package_location(
        control,
        control_src,
        control_uri,
        &package_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;
    use debian_control::lossless::Control;

    fn write_lintian_overrides(dir: &std::path::Path, content: &str) -> Uri {
        let source_dir = dir.join("debian").join("source");
        std::fs::create_dir_all(&source_dir).unwrap();
        let path = source_dir.join("lintian-overrides");
        std::fs::write(&path, content).unwrap();
        Uri::from_file_path(&path).unwrap()
    }

    fn write_debian_control(dir: &std::path::Path, content: &str) -> (Uri, String) {
        let debian_dir = dir.join("debian");
        std::fs::create_dir_all(&debian_dir).unwrap();
        let path = debian_dir.join("control");
        std::fs::write(&path, content).unwrap();
        (Uri::from_file_path(&path).unwrap(), content.to_string())
    }

    fn call_goto(
        content: &str,
        control_content: &str,
        control_uri: &Uri,
        position: Position,
    ) -> Option<Location> {
        let parsed = LintianOverrides::parse(content);
        let overrides = parsed.tree();
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let control_parsed = Control::parse(control_content);
        let control = control_parsed.tree();
        let control_idx = LineIndex::new(control_content);
        let control_src = Source::new(control_content, &control_idx);

        goto_definition(
            &overrides,
            src,
            position,
            &control,
            control_src,
            control_uri,
        )
    }

    const DEBIAN_CONTROL: &str = "\
Source: curl
Maintainer: Test <test@example.com>

Package: libcurl4
Architecture: any
Description: curl library
";

    #[test]
    fn test_goto_package_name() {
        let dir = tempfile::tempdir().unwrap();
        let content = "libcurl4: hardening-no-pie\n";
        write_lintian_overrides(dir.path(), content);
        let (control_uri, control_text) = write_debian_control(dir.path(), DEBIAN_CONTROL);

        let result = call_goto(content, &control_text, &control_uri, Position::new(0, 3));
        assert!(result.is_some(), "Should find definition for libcurl4");
        assert_eq!(result.unwrap().range.start.line, 3);
    }

    #[test]
    fn test_goto_unknown_package_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "unknown-pkg: hardening-no-pie\n";
        write_lintian_overrides(dir.path(), content);
        let (control_uri, control_text) = write_debian_control(dir.path(), DEBIAN_CONTROL);

        let result = call_goto(content, &control_text, &control_uri, Position::new(0, 3));
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_on_tag_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "libcurl4: hardening-no-pie\n";
        write_lintian_overrides(dir.path(), content);
        let (control_uri, control_text) = write_debian_control(dir.path(), DEBIAN_CONTROL);

        // Cursor on "hardening-no-pie" (col 14)
        let result = call_goto(content, &control_text, &control_uri, Position::new(0, 14));
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_on_type_keyword_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "source: hardening-no-pie\n";
        write_lintian_overrides(dir.path(), content);
        let (control_uri, control_text) = write_debian_control(dir.path(), DEBIAN_CONTROL);

        // Cursor on "source" is PACKAGE_TYPE, not PACKAGE_NAME
        let result = call_goto(content, &control_text, &control_uri, Position::new(0, 3));
        assert!(result.is_none());
    }
}
