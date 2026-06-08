//! Index a `debian/copyright` (DEP-5) file into SCIP documents.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use debian_copyright::lossless::Copyright;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};

/// Indexed result for `debian/copyright`.
pub struct CopyrightIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index a `debian/copyright` file.
///
/// `source_name` and `version` are used to scope license short-name symbols to
/// this source package's index.
pub fn index(
    text: &str,
    relative_path: &str,
    source_name: &str,
    version: Option<&str>,
) -> CopyrightIndex {
    let (cp, _errors) = Copyright::from_str_relaxed(text).unwrap_or_else(|_| {
        // Fallback: empty document.
        (Copyright::empty(), Vec::new())
    });
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    // Syntax-highlighting occurrences for the whole document.
    occurrences.extend(crate::scip::highlight::deb822(cp.as_deb822(), &lines));

    // Clickable links for the Format header URI plus URLs embedded in the prose
    // fields (Comment, Disclaimer, Source, License text, ...).
    let mut url_symbols_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    crate::scip::links::emit_deb822(
        cp.as_deb822(),
        crate::copyright::fields::COPYRIGHT_FIELDS,
        text,
        &lines,
        &mut occurrences,
        &mut symbols_info,
        &mut url_symbols_seen,
    );

    // Documented field-name symbols for every paragraph.
    crate::scip::fields::emit_field_symbols(
        cp.as_deb822(),
        &lines,
        &mut occurrences,
        &mut symbols_info,
        |field| symbols::copyright_field(source_name, version, field),
        |field| {
            crate::deb822::completion::field_description(
                crate::copyright::fields::COPYRIGHT_FIELDS,
                field,
            )
        },
    );

    // Definitions: standalone License paragraphs.
    for lic in cp.iter_licenses() {
        let Some(name) = lic.name() else { continue };
        let sym = symbols::license(source_name, version, &name);
        // The whole License paragraph is the enclosing scope.
        let stanza = lic.as_deb822().text_range();
        let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());
        if let Some(entry) = lic.as_deb822().get_entry("License") {
            if let Some(tr) = entry.value_token_range() {
                occurrences.push(Occurrence {
                    range: lines.range(tr.start().into(), tr.end().into()),
                    symbol: sym.clone(),
                    symbol_roles: SymbolRole::Definition as i32,
                    syntax_kind: scip::types::SyntaxKind::IdentifierType.into(),
                    enclosing_range,
                    ..Default::default()
                });
            }
        }
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Class.into(),
            display_name: name.clone(),
            ..Default::default()
        });
    }

    // References: per-Files paragraphs that cite a license short-name, plus
    // a definition for the Files: glob pattern itself.
    for fp in cp.iter_files() {
        // The license short-name cited by this paragraph, if any.
        let license_name = fp
            .license()
            .and_then(|lic| lic.name().map(|n| n.to_owned()));

        // The whole Files paragraph is the enclosing scope of its glob.
        let stanza = fp.as_deb822().text_range();
        let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());

        // Files: glob definition.
        let files = fp.files();
        if !files.is_empty() {
            let glob_key = files.join(" ");
            let glob_sym = symbols::copyright_files_glob(source_name, version, &glob_key);
            if let Some(entry) = fp.as_deb822().get_entry("Files") {
                if let Some(vr) = entry.value_range() {
                    occurrences.push(Occurrence {
                        range: lines.range(vr.start().into(), vr.end().into()),
                        symbol: glob_sym.clone(),
                        symbol_roles: SymbolRole::Definition as i32,
                        syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
                        enclosing_range: enclosing_range.clone(),
                        ..Default::default()
                    });
                    // The glob references the license that applies to it, so
                    // "find references" on a license surfaces the file globs
                    // it covers.
                    let relationships = match &license_name {
                        Some(name) => {
                            vec![symbols::rel_reference(symbols::license(
                                source_name,
                                version,
                                name,
                            ))]
                        }
                        None => Vec::new(),
                    };
                    symbols_info.push(SymbolInformation {
                        symbol: glob_sym,
                        kind: scip::types::symbol_information::Kind::File.into(),
                        display_name: glob_key.clone(),
                        relationships,
                        ..Default::default()
                    });
                }
            }
        }

        // License reference occurrence.
        let Some(name) = license_name else { continue };
        let sym = symbols::license(source_name, version, &name);
        if let Some(entry) = fp.as_deb822().get_entry("License") {
            if let Some(tr) = entry.value_token_range() {
                occurrences.push(Occurrence {
                    range: lines.range(tr.start().into(), tr.end().into()),
                    symbol: sym,
                    syntax_kind: scip::types::SyntaxKind::IdentifierType.into(),
                    ..Default::default()
                });
            }
        }
    }

    CopyrightIndex {
        document: Document {
            language: "debcopyright".to_owned(),
            relative_path: relative_path.to_owned(),
            text: text.to_owned(),
            occurrences,
            symbols: symbols_info,
            position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                .into(),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2026 Jelmer Vernooĳ <jelmer@debian.org>
License: GPL-2+

Files: vendor/*
Copyright: 2024 Upstream
License: MIT

License: GPL-2+
 This program is free software; you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation; either version 2 of the License, or
 (at your option) any later version.

License: MIT
 Permission is hereby granted, free of charge...
";

    #[test]
    fn indexes_license_defs_and_refs() {
        let idx = index(SAMPLE, "debian/copyright", "hello", Some("2.10-3"));
        // License-stanza and Files-glob definitions, excluding the
        // field-name definitions emitted for documentation.
        let field_syms: std::collections::HashSet<String> =
            crate::copyright::fields::COPYRIGHT_FIELDS
                .iter()
                .map(|f| symbols::copyright_field("hello", Some("2.10-3"), f.name))
                .collect();
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                (o.symbol_roles & SymbolRole::Definition as i32) != 0
                    && !field_syms.contains(&o.symbol)
            })
            .collect();
        // 2 license stanzas + 2 Files: globs = 4 definitions.
        assert_eq!(defs.len(), 4);

        // License reference occurrences carry a symbol but no Definition role.
        // Filter out the symbol-less syntax-highlighting occurrences and the
        // URL link occurrences.
        let refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                !o.symbol.is_empty()
                    && (o.symbol_roles & SymbolRole::Definition as i32) == 0
                    && o.symbol.contains("license")
            })
            .collect();
        assert_eq!(refs.len(), 2);

        // The `Format:` header URI is surfaced as a clickable link.
        let format_url = "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";
        let link = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == symbols::web_url(format_url))
            .expect("Format URL link occurrence");
        assert_eq!(link.symbol_roles & SymbolRole::Definition as i32, 0);
        let link_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == symbols::web_url(format_url))
            .expect("Format URL symbol info");
        assert_eq!(
            link_sym.documentation,
            vec![symbols::web_url_doc_labeled("Format", format_url)]
        );

        // Symbols on def and ref for GPL-2+ should match.
        let gpl_def = defs
            .iter()
            .find(|o| o.symbol.contains("license") && o.symbol.contains("GPL-2+"))
            .expect("GPL-2+ def");
        let gpl_ref = refs
            .iter()
            .find(|o| o.symbol.contains("GPL-2+"))
            .expect("GPL-2+ ref");
        assert_eq!(gpl_def.symbol, gpl_ref.symbol);

        // The `Files: *` glob references the GPL-2+ license it cites.
        let glob_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == symbols::copyright_files_glob("hello", Some("2.10-3"), "*"))
            .expect("Files glob symbol info");
        assert_eq!(glob_sym.relationships.len(), 1);
        assert_eq!(
            glob_sym.relationships[0].symbol,
            symbols::license("hello", Some("2.10-3"), "GPL-2+")
        );
        assert!(glob_sym.relationships[0].is_reference);

        // The `Files: vendor/*` glob references MIT.
        let vendor_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| {
                s.symbol == symbols::copyright_files_glob("hello", Some("2.10-3"), "vendor/*")
            })
            .expect("vendor glob symbol info");
        assert_eq!(
            vendor_sym.relationships[0].symbol,
            symbols::license("hello", Some("2.10-3"), "MIT")
        );
    }

    #[test]
    fn field_names_carry_documentation() {
        let idx = index(SAMPLE, "debian/copyright", "hello", Some("2.10-3"));

        // The `Format` header field is a documented symbol matching the LSP
        // hover description.
        let format_sym = symbols::copyright_field("hello", Some("2.10-3"), "Format");
        let format = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == format_sym)
            .expect("Format field symbol");
        assert_eq!(
            format.documentation,
            vec![crate::deb822::completion::field_description(
                crate::copyright::fields::COPYRIGHT_FIELDS,
                "Format"
            )
            .unwrap()
            .1
            .to_owned()]
        );

        // `License` recurs across paragraphs but is documented exactly once,
        // while every key occurrence still points at the one symbol.
        let license_sym = symbols::copyright_field("hello", Some("2.10-3"), "License");
        let infos = idx
            .document
            .symbols
            .iter()
            .filter(|s| s.symbol == license_sym)
            .count();
        assert_eq!(infos, 1);
        let occs = idx
            .document
            .occurrences
            .iter()
            .filter(|o| o.symbol == license_sym)
            .count();
        // Two Files paragraphs + two standalone License paragraphs.
        assert_eq!(occs, 4);
    }
}
