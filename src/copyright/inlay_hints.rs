//! Inlay hints for debian/copyright files.
//!
//! Shows the number of Files paragraphs that reference each standalone
//! License paragraph:
//! - `License: MIT (used by 3 Files paragraphs)`

use std::collections::HashMap;

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::text_range_to_lsp_range;

/// Generate inlay hints for copyright license paragraphs.
///
/// For each standalone License paragraph, counts how many Files paragraphs
/// reference that license name and displays the count as an inlay hint.
pub fn generate_inlay_hints(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
) -> Vec<InlayHint> {
    let copyright = parsed.to_copyright();
    let mut hints = Vec::new();

    let text_range = match crate::position::try_lsp_range_to_text_range(source_text, range) {
        Some(r) => r,
        None => return hints,
    };

    // Count how many Files paragraphs use each license name
    let mut license_usage: HashMap<String, usize> = HashMap::new();
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(name) = license.name() {
                let key = name.to_lowercase();
                *license_usage.entry(key).or_insert(0) += 1;
            }
        }
    }

    // Add hints to standalone License paragraphs within the requested range
    for license_para in copyright.iter_licenses() {
        let para = license_para.as_deb822();
        let para_range = para.syntax().text_range();

        // Skip paragraphs outside the requested range
        if para_range.end() < text_range.start() || para_range.start() > text_range.end() {
            continue;
        }

        let Some(name) = license_para.name() else {
            continue;
        };

        let key = name.to_lowercase();
        let count = license_usage.get(&key).copied().unwrap_or(0);

        let hint_text = match count {
            0 => "unused".to_string(),
            1 => "used by 1 Files paragraph".to_string(),
            n => format!("used by {n} Files paragraphs"),
        };

        // Place the hint at the end of the License field value (first line of the paragraph)
        // Find the end of the "License: <name>" portion
        let para_text = para.syntax().text().to_string();
        let hint_offset = if let Some(colon_pos) = para_text.find(':') {
            // Skip ": " then the license name
            let value_start = colon_pos + 1;
            let first_line = &para_text[value_start..];
            let line_len = first_line.find('\n').unwrap_or(first_line.len());
            value_start + line_len
        } else {
            para_text.find('\n').unwrap_or(para_text.len())
        };

        let abs_offset = para_range.start() + text_size::TextSize::from(hint_offset as u32);
        let lsp_range = text_range_to_lsp_range(
            source_text,
            text_size::TextRange::new(abs_offset, abs_offset),
        );

        hints.push(InlayHint {
            position: lsp_range.start,
            label: InlayHintLabel::String(hint_text),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_copyright::lossless::Parse;

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    fn range_for(text: &str) -> tower_lsp_server::ls_types::Range {
        let lines = text.lines().count() as u32;
        tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(lines, 0),
        }
    }

    #[test]
    fn test_license_used_by_multiple_files() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: lib/*
Copyright: 2024 Bob
License: MIT

Files: debian/*
Copyright: 2024 Carol
License: GPL-2+

License: MIT
 Permission is hereby granted...

License: GPL-2+
 This program is free software...
";
        let parsed = parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 2);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "used by 2 Files paragraphs"),
            _ => panic!("Expected string label"),
        }

        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "used by 1 Files paragraph"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_unused_license() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT

License: MIT
 Permission is hereby granted...

License: Apache-2.0
 Licensed under the Apache License...
";
        let parsed = parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 2);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "used by 1 Files paragraph"),
            _ => panic!("Expected string label"),
        }

        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "unused"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_no_standalone_licenses() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: mit

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 1);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "used by 1 Files paragraph"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_empty_copyright() {
        let text = "";
        let parsed = parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 0);
    }
}
