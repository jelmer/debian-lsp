//! Shared spell-checking helpers for deb822-based files.
//!
//! Both `debian/control` and `debian/copyright` are deb822 documents whose
//! prose lives in field values. This module walks the paragraphs of such a
//! document, checks the values of the fields a caller designates as prose, and
//! maps each typo back to a source range. The per-file wiring (which fields are
//! prose) lives next to each file type, e.g. [`crate::control::spelling`] and
//! [`crate::copyright::spelling`].

use deb822_lossless::Deb822;

use crate::control::inlay_hints::joined_offset_to_source_offset;
use crate::position::Source;
use crate::spelling::{check_text, LocatedFinding};

/// Find all spelling mistakes in the prose fields of a deb822 document, each
/// mapped to its source range. A field is prose when `is_prose_field` returns
/// true for its name.
pub fn deb822_findings(
    deb822: &Deb822,
    src: Source<'_>,
    is_prose_field: impl Fn(&str) -> bool,
) -> Vec<LocatedFinding> {
    let mut findings = Vec::new();

    for paragraph in deb822.paragraphs() {
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
