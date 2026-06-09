//! Go-to-definition for license names in `debian/copyright`.
//!
//! Clicking a license short-name inside a Files paragraph's License field
//! jumps to the matching standalone License paragraph. Compound expressions
//! like `MIT or Apache-2.0` resolve each name independently.

use debian_copyright::lossless::Parse;
use debian_copyright::LicenseExpr;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Location, Position, Uri};

use crate::position::Source;

/// Resolve go-to-definition for a license short-name cited in a Files
/// paragraph.
///
/// Returns a `Location` pointing at the matching standalone License
/// paragraph, or `None` if the cursor is not on a license name or no matching
/// definition exists in the file.
pub fn goto_definition(
    parse: &Parse,
    src: Source<'_>,
    position: Position,
    uri: &Uri,
) -> Option<Location> {
    let offset: u32 = src.try_position_to_offset(position)?.into();
    let copyright = parse.tree();

    let name = license_name_at_offset(&copyright, src.text, offset)?;

    // Look up the standalone License paragraph with that name.
    for license_para in copyright.iter_licenses() {
        if license_para.name().as_deref() == Some(name.as_str()) {
            let para = license_para.as_deb822();
            let range = src.text_range_to_lsp_range(para.syntax().text_range());
            return Some(Location {
                uri: uri.clone(),
                range,
            });
        }
    }
    None
}

fn license_name_at_offset(
    copyright: &debian_copyright::lossless::Copyright,
    text: &str,
    offset: u32,
) -> Option<String> {
    for fp in copyright.iter_files() {
        let entry = fp.as_deb822().get_entry("License")?;
        let vr = entry.value_range()?;
        let start: u32 = vr.start().into();
        let end: u32 = vr.end().into();
        if offset < start || offset > end {
            continue;
        }
        let value_text = &text[start as usize..end as usize];
        let offset_in_value = (offset - start) as usize;
        for (name, range) in LicenseExpr::name_ranges(value_text) {
            if range.start <= offset_in_value && offset_in_value <= range.end {
                return Some(name.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn test_uri() -> Uri {
        if cfg!(windows) {
            Uri::from_file_path("C:\\tmp\\debian\\copyright").unwrap()
        } else {
            Uri::from_file_path("/tmp/debian/copyright").unwrap()
        }
    }

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    const COMPOUND_SAMPLE: &str = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Alice
License: MIT or Apache-2.0

License: Apache-2.0
 Licensed under the Apache License, Version 2.0.

License: MIT
 Permission is hereby granted...
";

    #[test]
    fn jumps_from_first_name_in_or_expression() {
        let parsed = parse(COMPOUND_SAMPLE);
        let idx = LineIndex::new(COMPOUND_SAMPLE);
        let src = Source::new(COMPOUND_SAMPLE, &idx);
        // "License: " is 9 cols; "MIT" spans cols 9..12 on line 4.
        let result = goto_definition(&parsed, src, Position::new(4, 10), &test_uri());
        let loc = result.expect("MIT should resolve to the standalone License paragraph");
        assert_eq!(loc.range.start.line, 9);
    }

    #[test]
    fn jumps_from_second_name_in_or_expression() {
        let parsed = parse(COMPOUND_SAMPLE);
        let idx = LineIndex::new(COMPOUND_SAMPLE);
        let src = Source::new(COMPOUND_SAMPLE, &idx);
        // "Apache-2.0" spans cols 16..26 on line 4.
        let result = goto_definition(&parsed, src, Position::new(4, 20), &test_uri());
        let loc = result.expect("Apache-2.0 should resolve");
        assert_eq!(loc.range.start.line, 6);
    }

    #[test]
    fn cursor_off_a_name_returns_none() {
        let parsed = parse(COMPOUND_SAMPLE);
        let idx = LineIndex::new(COMPOUND_SAMPLE);
        let src = Source::new(COMPOUND_SAMPLE, &idx);
        // Cursor on the "or" between names.
        let result = goto_definition(&parsed, src, Position::new(4, 14), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn cursor_outside_license_field_returns_none() {
        let parsed = parse(COMPOUND_SAMPLE);
        let idx = LineIndex::new(COMPOUND_SAMPLE);
        let src = Source::new(COMPOUND_SAMPLE, &idx);
        // Cursor on Copyright line.
        let result = goto_definition(&parsed, src, Position::new(3, 5), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn unresolved_license_returns_none() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Alice
License: Made-Up-License
";
        let parsed = parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let result = goto_definition(&parsed, src, Position::new(4, 12), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn with_exception_resolves_head_only() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Alice
License: GPL-2+ with OpenSSL-exception

License: GPL-2+
 Licensed under the GNU GPL...
";
        let parsed = parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        // Cursor on "GPL-2+".
        let result = goto_definition(&parsed, src, Position::new(4, 11), &test_uri());
        let loc = result.expect("GPL-2+ should resolve");
        assert_eq!(loc.range.start.line, 6);

        // Cursor on the exception name itself — not a license name.
        let result = goto_definition(&parsed, src, Position::new(4, 20), &test_uri());
        assert!(result.is_none());
    }
}
