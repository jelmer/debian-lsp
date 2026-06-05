//! Spell-checking for prose fields in `debian/control`.
//!
//! The generic checker lives in [`crate::spelling`]; this module supplies the
//! control-specific wiring: which fields hold free-form prose worth checking,
//! and how to map typo offsets in a joined field value back to source ranges.
//! Only free-text field values (e.g. `Description`) are checked, never field
//! names, package relationships or URLs.

use tower_lsp_server::ls_types::Diagnostic;

use crate::control::inlay_hints::joined_offset_to_source_offset;
use crate::position::Source;
use crate::spelling::{check_text, make_diagnostic, LocatedFinding};

/// Field names in `debian/control` whose values are free-form prose worth
/// spell-checking. Everything else (relationships, URLs, package names,
/// architectures) is skipped to avoid false positives.
fn is_prose_field(field_name: &str) -> bool {
    field_name.eq_ignore_ascii_case("Description")
}

/// Find all spelling mistakes in the prose fields of a `debian/control` file,
/// each mapped to its source range.
fn control_findings(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    let control = parsed.tree();
    let mut findings = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let Some(field_name) = entry.key() else {
                continue;
            };
            if !is_prose_field(&field_name) {
                continue;
            }

            let value = entry.value();
            let line_ranges = entry.value_line_ranges();

            for finding in check_text(&value) {
                // Map the typo span back from the joined value string to
                // absolute source offsets. `entry.value()` joins continuation
                // lines with '\n', so the mapping is not a simple shift.
                let span = finding.span();
                let (Some(start), Some(end)) = (
                    joined_offset_to_source_offset(&line_ranges, span.start),
                    joined_offset_to_source_offset(&line_ranges, span.end),
                ) else {
                    continue;
                };

                let range = src.text_range_to_lsp_range(text_size::TextRange::new(start, end));
                findings.push(LocatedFinding { range, finding });
            }
        }
    }

    findings
}

/// Produce spelling diagnostics for the prose fields of a `debian/control`
/// file.
pub fn control_diagnostics(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    src: Source<'_>,
) -> Vec<Diagnostic> {
    control_findings(parsed, src)
        .into_iter()
        .map(|located| make_diagnostic(located.range, &located.finding))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{Position, Range};

    fn control_diags(content: &str) -> Vec<Diagnostic> {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        control_diagnostics(&parsed, src)
    }

    #[test]
    fn test_control_diagnostics_description_typo() {
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: a libary for things\n";
        let diags = control_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"libary\" should be \"library\"");
        // "libary" starts at column 15 on the Description line (line 4).
        assert_eq!(
            diags[0].range,
            Range::new(Position::new(4, 15), Position::new(4, 21))
        );
    }

    #[test]
    fn test_control_diagnostics_skips_non_prose_fields() {
        // A typo-shaped token in a non-prose field must not be flagged.
        let content = "Source: foo\nBuild-Depends: libary-dev\n\nPackage: foo\nArchitecture: all\nDescription: clean text\n";
        assert_eq!(control_diags(content), vec![]);
    }

    #[test]
    fn test_control_diagnostics_multiline_description() {
        // Typo on a continuation line: the joined-value offset must map back
        // through the '\n' separator to the right source position.
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: short\n A longer paragraph that recieves input.\n";
        let diags = control_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"recieves\" should be \"receives\"");
        assert_eq!(diags[0].range.start.line, 5);
    }
}
