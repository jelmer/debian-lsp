//! Shared emission of documented field-key symbols for deb822 SCIP indexers.
//!
//! Each deb822-based file format already carries a table of field descriptions
//! used by the LSP hover provider. This module reuses that data to attach the
//! same documentation to SCIP symbols, so hovering a field name in a
//! Sourcegraph-style consumer explains what the field is for, exactly as the
//! editor hover does.

use crate::scip::linetable::LineTable;
use scip::types::{Occurrence, SymbolInformation, SymbolRole};
use std::collections::HashSet;

/// Emit documented definition symbols for the known field keys of every
/// paragraph in a deb822 document.
///
/// Convenience wrapper around [`emit_paragraph_field_symbols`] for formats
/// whose fields share a single scope (e.g. `debian/copyright`,
/// `debian/tests/control`, a v5 `debian/watch`). For `debian/control`, where
/// source- and binary-stanza fields are scoped differently, call the
/// per-paragraph primitive directly.
pub fn emit_field_symbols(
    deb822: &deb822_lossless::Deb822,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    field_sym: impl Fn(&str) -> String,
    lookup: impl Fn(&str) -> Option<(&'static str, &'static str)>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    for paragraph in deb822.paragraphs() {
        emit_paragraph_field_symbols(
            &paragraph,
            lines,
            occurrences,
            symbols_info,
            &mut seen,
            &field_sym,
            &lookup,
        );
    }
}

/// Emit a documented definition symbol for each known field key in a single
/// deb822 paragraph.
///
/// `field_sym` builds the symbol identifier for a (canonical) field name, and
/// `lookup` returns the canonical name and description for a field (typically
/// [`crate::deb822::completion::field_description`] bound to a format's field
/// table). Fields without a known description are left to the syntax-highlight
/// pass and not turned into symbols.
///
/// A definition occurrence is emitted for every matching field key, but each
/// distinct symbol gets a single [`SymbolInformation`] entry: `seen` tracks the
/// symbols already documented, so a field name that recurs (across paragraphs
/// or stanzas) is documented once while every occurrence still points at it.
///
/// Each field definition carries the enclosing paragraph (stanza) as its
/// `enclosing_range`, so a consumer can fold or select the whole stanza from the
/// field name.
pub fn emit_paragraph_field_symbols(
    paragraph: &deb822_lossless::Paragraph,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    seen: &mut HashSet<String>,
    field_sym: impl Fn(&str) -> String,
    lookup: impl Fn(&str) -> Option<(&'static str, &'static str)>,
) {
    let stanza = paragraph.text_range();
    let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());
    for entry in paragraph.entries() {
        let Some(key) = entry.key() else { continue };
        let Some((canonical, description)) = lookup(&key) else {
            continue;
        };
        let Some(range) = entry.key_range() else {
            continue;
        };
        let sym = field_sym(canonical);
        occurrences.push(Occurrence {
            range: lines.range(range.start().into(), range.end().into()),
            symbol: sym.clone(),
            symbol_roles: SymbolRole::Definition as i32,
            enclosing_range: enclosing_range.clone(),
            ..Default::default()
        });
        if seen.insert(sym.clone()) {
            symbols_info.push(SymbolInformation {
                symbol: sym,
                kind: scip::types::symbol_information::Kind::Field.into(),
                display_name: canonical.to_owned(),
                documentation: vec![description.to_owned()],
                ..Default::default()
            });
        }
    }
}
