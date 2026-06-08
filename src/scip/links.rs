//! Surface web URLs in packaging metadata as clickable SCIP links.
//!
//! URL-valued fields (`Homepage`, `Vcs-*` in `debian/control`, `Format` in
//! `debian/copyright`, ...) link their whole value; prose fields (`Comment`,
//! `Disclaimer`, license text, ...) are scanned for embedded URLs with
//! [`crate::links::find_urls`]. Field classification comes from the shared
//! [`FieldContent`] metadata, so the LSP and SCIP agree on what is a link. Each
//! URL becomes a SCIP occurrence plus a [`SymbolInformation`] carrying the URL
//! as markdown documentation, so the SCIP consumer renders a navigable link.

use crate::deb822::completion::{field_content, FieldContent, FieldInfo};
use crate::links::find_urls;
use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use scip::types::{Occurrence, SymbolInformation, SymbolRole};
use std::collections::HashSet;

/// Emit a single link occurrence (and, on first sight, its symbol info) for the
/// URL spanning the byte range `span` in the document. `symbols_seen`
/// deduplicates the symbol info so a URL repeated in the file is documented once.
///
/// `label` is the originating field name (e.g. `Homepage`, `Vcs-Browser`) when
/// the URL is a whole field value; it becomes the link text so a consumer can
/// tell what the link is. `None` for URLs scraped from prose.
pub fn emit_url(
    url: &str,
    label: Option<&str>,
    span: std::ops::Range<u32>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    symbols_seen: &mut HashSet<String>,
) {
    let sym = symbols::web_url(url);
    occurrences.push(Occurrence {
        range: lines.range(span.start, span.end),
        symbol: sym.clone(),
        symbol_roles: SymbolRole::ReadAccess as i32,
        syntax_kind: scip::types::SyntaxKind::IdentifierConstant.into(),
        ..Default::default()
    });
    if symbols_seen.insert(sym.clone()) {
        let documentation = match label {
            Some(label) => symbols::web_url_doc_labeled(label, url),
            None => symbols::web_url_doc(url),
        };
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Constant.into(),
            display_name: url.to_owned(),
            documentation: vec![documentation],
            ..Default::default()
        });
    }
}

/// Link the whole `text` as a single URL, trimming surrounding whitespace.
///
/// `base` is the byte offset of `text` within the document. Used for fields
/// whose entire value is a URL.
fn emit_whole(
    text: &str,
    label: &str,
    base: u32,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    symbols_seen: &mut HashSet<String>,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let offset = text.find(trimmed).unwrap_or(0);
    let abs_start = base + offset as u32;
    let abs_end = abs_start + trimmed.len() as u32;
    emit_url(
        trimmed,
        Some(label),
        abs_start..abs_end,
        lines,
        occurrences,
        symbols_info,
        symbols_seen,
    );
}

/// Scan `text` for embedded URLs and emit a link for each, for prose fields.
///
/// `base` is the byte offset of `text` within the document.
fn emit_prose(
    text: &str,
    base: u32,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    symbols_seen: &mut HashSet<String>,
) {
    for (rel_start, rel_end) in find_urls(text) {
        let url = &text[rel_start..rel_end];
        emit_url(
            url,
            None,
            (base + rel_start as u32)..(base + rel_end as u32),
            lines,
            occurrences,
            symbols_info,
            symbols_seen,
        );
    }
}

/// Scan every field of a parsed deb822 document (`debian/control`,
/// `debian/copyright`) and emit link occurrences. `fields` classifies each field
/// as URL-valued, prose, or plain; only the first two yield links. `text` is the
/// whole document, used to slice each field's value range.
pub fn emit_deb822(
    deb822: &deb822_lossless::Deb822,
    fields: &[FieldInfo],
    text: &str,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    symbols_seen: &mut HashSet<String>,
) {
    for para in deb822.paragraphs() {
        for entry in para.entries() {
            let Some(key) = entry.key() else { continue };
            let content = field_content(fields, &key);
            if content == FieldContent::Plain {
                continue;
            }
            let Some(vr) = entry.value_range() else {
                continue;
            };
            let start = u32::from(vr.start());
            let end = u32::from(vr.end());
            let value = &text[start as usize..end as usize];
            match content {
                FieldContent::Url => emit_whole(
                    value,
                    &key,
                    start,
                    lines,
                    occurrences,
                    symbols_info,
                    symbols_seen,
                ),
                FieldContent::Prose => {
                    emit_prose(value, start, lines, occurrences, symbols_info, symbols_seen)
                }
                FieldContent::Plain => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::fields::CONTROL_FIELDS;
    use crate::copyright::fields::COPYRIGHT_FIELDS;

    fn run(deb822: &deb822_lossless::Deb822, fields: &[FieldInfo], text: &str) -> Vec<String> {
        let lines = LineTable::new(text);
        let mut occ = Vec::new();
        let mut sym = Vec::new();
        let mut seen = HashSet::new();
        emit_deb822(deb822, fields, text, &lines, &mut occ, &mut sym, &mut seen);
        occ.iter().map(|o| o.symbol.clone()).collect()
    }

    #[test]
    fn links_url_field_whole_value() {
        let text = "Homepage: https://example.org/hello\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let syms = run(&deb822, CONTROL_FIELDS, text);
        assert_eq!(syms, vec![symbols::web_url("https://example.org/hello")]);
    }

    #[test]
    fn scans_prose_field_for_embedded_url() {
        let text = "Description: a tool\n See https://example.org/x for more.\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let syms = run(&deb822, CONTROL_FIELDS, text);
        assert_eq!(syms, vec![symbols::web_url("https://example.org/x")]);
    }

    #[test]
    fn prose_url_doc_is_unlabeled() {
        // A URL scraped from prose has no originating field, so its doc is the
        // bare self-link rather than a labeled one.
        let text = "Description: a tool\n See https://example.org/x for more.\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let lines = LineTable::new(text);
        let mut occ = Vec::new();
        let mut sym = Vec::new();
        let mut seen = HashSet::new();
        emit_deb822(
            &deb822,
            CONTROL_FIELDS,
            text,
            &lines,
            &mut occ,
            &mut sym,
            &mut seen,
        );
        assert_eq!(
            sym[0].documentation,
            vec![symbols::web_url_doc("https://example.org/x")]
        );
    }

    #[test]
    fn ignores_plain_field() {
        let text = "Maintainer: Jelmer <https://example.org/me>\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let syms = run(&deb822, CONTROL_FIELDS, text);
        assert!(
            syms.is_empty(),
            "plain field should not be linked: {syms:?}"
        );
    }

    #[test]
    fn links_copyright_format_and_scans_comment() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Comment: derived from https://upstream.example/notice
";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let lines = LineTable::new(text);
        let mut occ = Vec::new();
        let mut sym = Vec::new();
        let mut seen = HashSet::new();
        emit_deb822(
            &deb822,
            COPYRIGHT_FIELDS,
            text,
            &lines,
            &mut occ,
            &mut sym,
            &mut seen,
        );
        let syms: HashSet<_> = occ.iter().map(|o| o.symbol.clone()).collect();
        assert!(syms.contains(&symbols::web_url(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
        )));
        assert!(syms.contains(&symbols::web_url("https://upstream.example/notice")));
    }

    #[test]
    fn url_field_emits_one_occurrence_one_symbol() {
        let text = "Homepage: https://example.org/hello\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let lines = LineTable::new(text);
        let mut occ = Vec::new();
        let mut sym = Vec::new();
        let mut seen = HashSet::new();
        emit_deb822(
            &deb822,
            CONTROL_FIELDS,
            text,
            &lines,
            &mut occ,
            &mut sym,
            &mut seen,
        );
        assert_eq!(occ.len(), 1);
        assert_eq!(sym.len(), 1);
        // The link text is the originating field name, not the bare URL.
        assert_eq!(
            sym[0].documentation,
            vec!["[Homepage](https://example.org/hello)".to_owned()]
        );
    }

    #[test]
    fn dedups_repeated_url_across_fields() {
        let text = "Homepage: https://x.example/\nVcs-Browser: https://x.example/\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let lines = LineTable::new(text);
        let mut occ = Vec::new();
        let mut sym = Vec::new();
        let mut seen = HashSet::new();
        emit_deb822(
            &deb822,
            CONTROL_FIELDS,
            text,
            &lines,
            &mut occ,
            &mut sym,
            &mut seen,
        );
        // Two occurrences (both highlighted) but one documented symbol.
        assert_eq!(occ.len(), 2);
        assert_eq!(sym.len(), 1);
    }
}
