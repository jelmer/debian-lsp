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

    // Link the people named in `Copyright` and `Upstream-Contact` to the same
    // cross-archive identity symbol as control Maintainer/Uploaders, so "find
    // references" on a person surfaces the files they hold copyright on.
    for para in cp.as_deb822().paragraphs() {
        for field in ["Copyright", "Upstream-Contact"] {
            let Some(entry) = para.get_entry(field) else {
                continue;
            };
            let Some(vr) = entry.value_range() else {
                continue;
            };
            let (start, end): (usize, usize) = (vr.start().into(), vr.end().into());
            for (email, rel_start, rel_end) in symbols::identity_emails(&text[start..end]) {
                occurrences.push(lines.identity_occurrence(
                    email,
                    (start + rel_start) as u32,
                    (start + rel_end) as u32,
                ));
            }
        }
    }

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

    // References: per-Files paragraphs that cite a license expression, plus
    // a definition for the Files: glob pattern itself.
    for fp in cp.iter_files() {
        // The whole Files paragraph is the enclosing scope of its glob.
        let stanza = fp.as_deb822().text_range();
        let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());

        // The license entry text and value range tell us where to look for
        // individual license names inside compound expressions like
        // `MIT or Apache-2.0`.
        let license_entry = fp.as_deb822().get_entry("License");
        let license_names: Vec<(String, u32, u32)> = license_entry
            .as_ref()
            .and_then(|entry| {
                let vr = entry.value_range()?;
                let start: u32 = vr.start().into();
                let end: u32 = vr.end().into();
                let value_text = &text[start as usize..end as usize];
                Some(
                    debian_copyright::LicenseExpr::name_ranges(value_text)
                        .into_iter()
                        .map(|(name, r)| {
                            (
                                name.to_owned(),
                                start + r.start as u32,
                                start + r.end as u32,
                            )
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();

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
                    // The glob references every license that applies to it, so
                    // "find references" on a license surfaces the file globs
                    // it covers.
                    let relationships = license_names
                        .iter()
                        .map(|(name, _, _)| {
                            symbols::rel_reference(symbols::license(source_name, version, name))
                        })
                        .collect();
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

        // One reference occurrence per license name in the expression.
        for (name, start, end) in &license_names {
            occurrences.push(Occurrence {
                range: lines.range(*start, *end),
                symbol: symbols::license(source_name, version, name),
                syntax_kind: scip::types::SyntaxKind::IdentifierType.into(),
                ..Default::default()
            });
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
    fn indexes_each_name_in_compound_expression() {
        // The Files paragraph cites two licenses via `or`. Both should be
        // highlighted and resolve to the standalone License paragraph defs.
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Alice
License: MIT or Apache-2.0

License: Apache-2.0
 Licensed under the Apache License, Version 2.0.

License: MIT
 Permission is hereby granted...
";
        let idx = index(text, "debian/copyright", "git2", Some("0.1-1"));

        let license_refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                !o.symbol.is_empty()
                    && (o.symbol_roles & SymbolRole::Definition as i32) == 0
                    && o.symbol.contains("license")
            })
            .collect();
        let ref_syms: Vec<&str> = license_refs.iter().map(|o| o.symbol.as_str()).collect();
        let mit_sym = symbols::license("git2", Some("0.1-1"), "MIT");
        let apache_sym = symbols::license("git2", Some("0.1-1"), "Apache-2.0");
        assert!(
            ref_syms.contains(&mit_sym.as_str()),
            "missing MIT ref, got {ref_syms:?}"
        );
        assert!(
            ref_syms.contains(&apache_sym.as_str()),
            "missing Apache-2.0 ref, got {ref_syms:?}"
        );

        // Each ref points at the actual name text in the source.
        let mit_ref = license_refs
            .iter()
            .find(|o| o.symbol == mit_sym)
            .expect("MIT ref");
        let apache_ref = license_refs
            .iter()
            .find(|o| o.symbol == apache_sym)
            .expect("Apache-2.0 ref");
        // The License field is on line 4 (zero-indexed). `License: ` is 9 cols,
        // so "MIT" spans cols 9..12 and "Apache-2.0" spans cols 16..26.
        assert_eq!(mit_ref.range, vec![4, 9, 4, 12]);
        assert_eq!(apache_ref.range, vec![4, 16, 4, 26]);

        // The glob lists both licenses among its relationships.
        let glob_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == symbols::copyright_files_glob("git2", Some("0.1-1"), "*"))
            .expect("Files glob symbol info");
        let rel_syms: Vec<&str> = glob_sym
            .relationships
            .iter()
            .map(|r| r.symbol.as_str())
            .collect();
        assert!(rel_syms.contains(&mit_sym.as_str()));
        assert!(rel_syms.contains(&apache_sym.as_str()));
    }

    #[test]
    fn indexes_name_in_with_exception() {
        // `with` introduces an exception that should be skipped — the only
        // license name is the head.
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Alice
License: GPL-2+ with OpenSSL-exception

License: GPL-2+
 Licensed under the GNU GPL...
";
        let idx = index(text, "debian/copyright", "pkg", Some("1-1"));

        let gpl_sym = symbols::license("pkg", Some("1-1"), "GPL-2+");
        let license_refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                !o.symbol.is_empty()
                    && (o.symbol_roles & SymbolRole::Definition as i32) == 0
                    && o.symbol.contains("license")
            })
            .collect();
        let ref_syms: Vec<&str> = license_refs.iter().map(|o| o.symbol.as_str()).collect();
        assert_eq!(ref_syms, vec![gpl_sym.as_str()]);
    }

    #[test]
    fn links_copyright_and_upstream_contact_emails() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Contact: Jane Doe <jane@example.org>

Files: *
Copyright: 2026 Jelmer Vernoo\u{133} <jelmer@debian.org>
License: GPL-2+

License: GPL-2+
 text
";
        let idx = index(text, "debian/copyright", "hello", Some("1-1"));

        let upstream = symbols::identity("jane@example.org");
        let up = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == upstream)
            .expect("Upstream-Contact email identity occurrence");
        // Line 1 (zero-indexed): `Upstream-Contact: Jane Doe <` is 28 cols, so
        // the email spans cols 28..44.
        assert_eq!(up.range, vec![1, 28, 1, 44]);

        let holder = symbols::identity("jelmer@debian.org");
        assert!(
            idx.document.occurrences.iter().any(|o| o.symbol == holder),
            "Copyright email should be linked to an identity symbol"
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
