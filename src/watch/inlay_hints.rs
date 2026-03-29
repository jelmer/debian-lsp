//! Inlay hints for debian/watch files.
//!
//! Shows the practical effect of version/URL mangle rules as inline hints.
//! For example, a `uversionmangle=s/\+ds//` rule will display:
//!   `[e.g. 1.2.3+ds → 1.2.3]`

use debian_watch::SyntaxKind;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel, Range};

use crate::position::text_range_to_lsp_range;

/// Mangle field names (lowercase) and the example input to demonstrate them with.
/// The inputs are chosen to be realistic and likely to be affected by common mangles.
const MANGLE_FIELDS: &[(&str, &str)] = &[
    ("uversionmangle", "1.2.3+ds"),
    ("oversionmangle", "1.2.3+ds"),
    ("dversionmangle", "1.2.3+dfsg-1"),
    ("dirversionmangle", "v1.2.3"),
    ("versionmangle", "1.2.3+ds"),
    (
        "downloadurlmangle",
        "https://example.com/project/archive/v1.2.3.tar.gz",
    ),
    (
        "filenamemangle",
        "https://example.com/project/archive/v1.2.3.tar.gz",
    ),
    (
        "pgpsigurlmangle",
        "https://example.com/project/archive/v1.2.3.tar.gz",
    ),
    ("pagemangle", "<a href=\"project-1.2.3.tar.gz\">"),
];

/// Look up the example input for a mangle field name (case-insensitive).
fn example_input_for(field_name: &str) -> Option<&'static str> {
    let lower = field_name.to_lowercase();
    MANGLE_FIELDS
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, input)| *input)
}

/// Try to apply a mangle expression and produce a hint label.
fn mangle_hint_label(mangle_expr: &str, example_input: &str) -> Option<String> {
    match debian_watch::mangle::apply_mangle(mangle_expr, example_input) {
        Ok(result) if result != example_input => {
            Some(format!("e.g. {} → {}", example_input, result))
        }
        _ => None,
    }
}

fn make_hint(position: tower_lsp_server::ls_types::Position, label: String) -> InlayHint {
    InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    }
}

/// Generate inlay hints for a watch file (both v1-4 and v5 formats).
pub fn generate_inlay_hints(
    parsed: &debian_watch::parse::Parse,
    source_text: &str,
    range: &Range,
) -> Vec<InlayHint> {
    let wf = parsed.to_watch_file();
    match &wf {
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            generate_linebased_hints(wf, source_text, range)
        }
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            generate_deb822_hints(wf.as_deb822(), source_text, range)
        }
    }
}

/// Generate hints for v1-4 line-based watch files by walking the CST.
fn generate_linebased_hints(
    wf: &debian_watch::linebased::WatchFile,
    source_text: &str,
    range: &Range,
) -> Vec<InlayHint> {
    let text_range = match crate::position::try_lsp_range_to_text_range(source_text, range) {
        Some(r) => r,
        None => return vec![],
    };

    let mut hints = Vec::new();

    for entry in wf.entries() {
        let entry_range = entry.syntax().text_range();
        if entry_range.end() < text_range.start() || entry_range.start() > text_range.end() {
            continue;
        }

        // Walk descendants to find OPTION nodes with mangle keys.
        // Each OPTION node has: KEY token, EQUALS token, VALUE token.
        for node in entry.syntax().descendants() {
            if node.kind() != SyntaxKind::OPTION {
                continue;
            }

            let mut key_text = None;
            let mut value_token_range = None;
            let mut value_text = None;

            for element in node.children_with_tokens() {
                if let rowan::NodeOrToken::Token(token) = element {
                    match token.kind() {
                        SyntaxKind::KEY if key_text.is_none() => {
                            key_text = Some(token.text().to_string());
                        }
                        SyntaxKind::VALUE | SyntaxKind::KEY
                            if key_text.is_some() && value_text.is_none() =>
                        {
                            value_text = Some(token.text().to_string());
                            value_token_range = Some(token.text_range());
                        }
                        _ => {}
                    }
                }
            }

            if let (Some(key), Some(value), Some(vrange)) =
                (key_text, value_text, value_token_range)
            {
                if let Some(example_input) = example_input_for(&key) {
                    if let Some(label) = mangle_hint_label(&value, example_input) {
                        let lsp_range = text_range_to_lsp_range(source_text, vrange);
                        hints.push(make_hint(lsp_range.end, label));
                    }
                }
            }
        }
    }

    hints
}

/// Generate hints for v5 deb822 watch files.
fn generate_deb822_hints(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    range: &Range,
) -> Vec<InlayHint> {
    let text_range = match crate::position::try_lsp_range_to_text_range(source_text, range) {
        Some(r) => r,
        None => return vec![],
    };

    let mut hints = Vec::new();

    for paragraph in deb822.paragraphs() {
        for entry in paragraph.entries() {
            let entry_text_range = entry.text_range();
            if entry_text_range.end() < text_range.start()
                || entry_text_range.start() > text_range.end()
            {
                continue;
            }

            let Some(field_name) = entry.key() else {
                continue;
            };
            let Some(example_input) = example_input_for(&field_name) else {
                continue;
            };

            let value = entry.value().to_string().trim().to_string();
            if value.is_empty() {
                continue;
            }

            if let Some(label) = mangle_hint_label(&value, example_input) {
                let lsp_range = text_range_to_lsp_range(source_text, entry_text_range);
                hints.push(make_hint(lsp_range.end, label));
            }
        }
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::Position;

    fn range_for(text: &str) -> Range {
        let lines = text.lines().count() as u32;
        Range {
            start: Position::new(0, 0),
            end: Position::new(lines, 0),
        }
    }

    #[test]
    fn test_linebased_uversionmangle() {
        let text = "version=4\nopts=uversionmangle=s/\\+ds// https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. 1.2.3+ds → 1.2.3"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_linebased_dversionmangle() {
        let text = "version=4\nopts=dversionmangle=s/\\+dfsg// https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. 1.2.3+dfsg-1 → 1.2.3-1"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_linebased_no_hint_when_noop() {
        let text = "version=4\nopts=uversionmangle=s/alpha// https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_linebased_non_mangle_option() {
        let text = "version=4\nopts=mode=git https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_deb822_uversionmangle() {
        let text = "Version: 5\n\nSource: https://example.com\nUversionmangle: s/\\+ds//\n\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. 1.2.3+ds → 1.2.3"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_deb822_filenamemangle() {
        let text =
            "Version: 5\n\nSource: https://example.com\nFilenamemangle: s/.+\\/v?(\\d\\S+)\\.tar\\.gz/pkg-$1.tar.gz/\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => {
                assert!(s.contains("→"), "Hint should contain arrow: {}", s);
                assert!(s.contains("pkg-"), "Hint should show mangled result: {}", s);
            }
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_range_filtering() {
        let text = "version=4\nopts=uversionmangle=s/\\+ds// https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);

        // Request only line 0 (the version line) - should not include hints from line 1
        let range = Range {
            start: Position::new(0, 0),
            end: Position::new(0, 10),
        };
        let hints = generate_inlay_hints(&parsed, text, &range);
        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_invalid_mangle_no_hint() {
        let text = "version=4\nopts=uversionmangle=not-a-mangle https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let hints = generate_inlay_hints(&parsed, text, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_mangle_hint_label_simple() {
        let label = mangle_hint_label("s/foo/bar/", "foo-1.2.3");
        assert_eq!(label, Some("e.g. foo-1.2.3 → bar-1.2.3".to_string()));
    }

    #[test]
    fn test_mangle_hint_label_noop() {
        let label = mangle_hint_label("s/foo/bar/", "baz-1.2.3");
        assert_eq!(label, None);
    }

    #[test]
    fn test_example_input_for_known_fields() {
        assert!(example_input_for("uversionmangle").is_some());
        assert!(example_input_for("Uversionmangle").is_some());
        assert!(example_input_for("dversionmangle").is_some());
        assert!(example_input_for("filenamemangle").is_some());
        assert!(example_input_for("mode").is_none());
    }
}
