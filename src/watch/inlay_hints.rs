//! Inlay hints for debian/watch files.
//!
//! Shows the practical effect of version/URL mangle rules as inline hints.
//! For example, a `uversionmangle=s/\+ds//` rule will display:
//!   `[e.g. 1.2.3+ds → 1.2.3]`

use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel, Range};

use crate::position::Source;

/// Mangle field names (lowercase) and the example input to demonstrate them with.
///
/// Inputs cover the common patterns each mangle is written for: a `v` prefix
/// stripped by `s/^v//`, `+ds`/`+dfsg`/`+really` repack markers, trailing `.0`
/// components trimmed by `s/\.0+$//`, and dashes-vs-dots translations.
const MANGLE_FIELDS: &[(&str, &str)] = &[
    ("uversionmangle", "v1.2.3+ds"),
    ("oversionmangle", "v1.2.3+ds"),
    ("dversionmangle", "1.2.3+dfsg-1"),
    ("dirversionmangle", "v1.2.3"),
    ("versionmangle", "v1.2.3+ds"),
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
    MANGLE_FIELDS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field_name))
        .map(|(_, input)| *input)
}

/// Split a compound mangle expression into its individual rules.
///
/// uscan supports chaining several mangle rules with `;`, e.g.
/// `s/^v//;s/\.0+$//`. Each rule is applied to the output of the previous
/// one. Backslash-escaped semicolons are kept as part of a rule.
fn split_mangle_rules(expr: &str) -> Vec<&str> {
    let mut rules = Vec::new();
    let mut start = 0;
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == b';' {
            let rule = expr[start..i].trim();
            if !rule.is_empty() {
                rules.push(rule);
            }
            start = i + 1;
        }
        i += 1;
    }
    let tail = expr[start..].trim();
    if !tail.is_empty() {
        rules.push(tail);
    }
    rules
}

/// Try to apply a (possibly compound) mangle expression and produce a hint label.
///
/// Returns `None` if the expression fails to parse or if the end result is
/// identical to the input (so there's nothing useful to show).
fn mangle_hint_label(mangle_expr: &str, example_input: &str) -> Option<String> {
    let mut current = example_input.to_string();
    for rule in split_mangle_rules(mangle_expr) {
        current = debian_watch::mangle::apply_mangle(rule, &current).ok()?;
    }
    if current == example_input {
        return None;
    }
    Some(format!("e.g. {} → {}", example_input, current))
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
    src: Source,
    range: &Range,
) -> Vec<InlayHint> {
    let wf = parsed.to_watch_file();
    match &wf {
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            generate_linebased_hints(wf, src, range)
        }
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            generate_deb822_hints(wf.as_deb822(), src, range)
        }
    }
}

/// Generate hints for v1-4 line-based watch files.
fn generate_linebased_hints(
    wf: &debian_watch::linebased::WatchFile,
    src: Source,
    range: &Range,
) -> Vec<InlayHint> {
    let Some(text_range) = src.try_lsp_range_to_text_range(range) else {
        return vec![];
    };

    let mut hints = Vec::new();

    for entry in wf.entries() {
        let entry_range = entry.syntax().text_range();
        if entry_range.end() < text_range.start() || entry_range.start() > text_range.end() {
            continue;
        }

        let Some(option_list) = entry.option_list() else {
            continue;
        };

        for option in option_list.options() {
            let (Some(key), Some(value)) = (option.key(), option.value()) else {
                continue;
            };
            let Some(example_input) = example_input_for(&key) else {
                continue;
            };
            let Some(label) = mangle_hint_label(&value, example_input) else {
                continue;
            };
            let lsp_range = src.text_range_to_lsp_range(option.text_range());
            hints.push(make_hint(lsp_range.end, label));
        }
    }

    hints
}

/// Generate hints for v5 deb822 watch files.
fn generate_deb822_hints(
    deb822: &deb822_lossless::Deb822,
    src: Source,
    range: &Range,
) -> Vec<InlayHint> {
    let Some(text_range) = src.try_lsp_range_to_text_range(range) else {
        return vec![];
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

            let value = entry.value();
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Some(label) = mangle_hint_label(trimmed, example_input) else {
                continue;
            };
            let anchor = entry.value_range().unwrap_or(entry_text_range);
            let lsp_range = src.text_range_to_lsp_range(anchor);
            hints.push(make_hint(lsp_range.end, label));
        }
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;
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
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. v1.2.3+ds → v1.2.3"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_linebased_dversionmangle() {
        let text = "version=4\nopts=dversionmangle=s/\\+dfsg// https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

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
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_linebased_non_mangle_option() {
        let text = "version=4\nopts=mode=git https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_deb822_uversionmangle() {
        let text = "Version: 5\n\nSource: https://example.com\nUversionmangle: s/\\+ds//\n\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. v1.2.3+ds → v1.2.3"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_deb822_filenamemangle() {
        let text =
            "Version: 5\n\nSource: https://example.com\nFilenamemangle: s/.+\\/v?(\\d\\S+)\\.tar\\.gz/pkg-$1.tar.gz/\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

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
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);

        // Request only line 0 (the version line) - should not include hints from line 1
        let range = Range {
            start: Position::new(0, 0),
            end: Position::new(0, 10),
        };
        let hints = generate_inlay_hints(&parsed, src, &range);
        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_invalid_mangle_no_hint() {
        let text = "version=4\nopts=uversionmangle=not-a-mangle https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));

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
    fn test_mangle_hint_label_compound() {
        // Two rules chained with `;` should be applied in sequence.
        let label = mangle_hint_label("s/^v//;s/\\.0+$//", "v1.2.0");
        assert_eq!(label, Some("e.g. v1.2.0 → 1.2".to_string()));
    }

    #[test]
    fn test_split_mangle_rules_escapes_semicolon() {
        // A backslash-escaped semicolon should stay inside its rule.
        assert_eq!(split_mangle_rules(r"s/a\;b/c/"), vec![r"s/a\;b/c/"]);
        assert_eq!(
            split_mangle_rules("s/a/b/;s/c/d/"),
            vec!["s/a/b/", "s/c/d/"]
        );
        assert_eq!(split_mangle_rules(";;s/a/b/;"), vec!["s/a/b/"]);
    }

    #[test]
    fn test_linebased_compound_mangle() {
        // Two mangle rules chained with `;`. Quotes keep the compound
        // expression inside a single opts entry.
        let text = "version=4\nopts=\"uversionmangle=s/^v//;s/\\+ds//\" https://example.com/ .*\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));
        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. v1.2.3+ds → 1.2.3"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_deb822_compound_mangle() {
        let text = "Version: 5\n\nSource: https://example.com\nUversionmangle: s/^v//;s/\\+ds//\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let hints = generate_inlay_hints(&parsed, src, &range_for(text));
        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "e.g. v1.2.3+ds → 1.2.3"),
            _ => panic!("Expected string label"),
        }
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
