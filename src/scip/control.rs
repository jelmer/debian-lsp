//! Index a `debian/control` file into SCIP documents.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use debian_control::lossless::Control;
use debian_control::{BINARY_RELATION_FIELDS, SOURCE_RELATION_FIELDS};
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};
use std::collections::HashSet;

/// Indexed result for a single `debian/control` document.
pub struct ControlIndex {
    /// The SCIP document, ready to be appended to an [`scip::types::Index`].
    pub document: Document,
    /// Names of external Debian binary packages referenced from this file.
    pub external_binaries: HashSet<String>,
    /// Build profile names referenced from relation fields (e.g. `nocheck`).
    pub build_profiles: HashSet<String>,
    /// Source package name as declared in the `Source:` field, if present.
    pub source_name: Option<String>,
    /// Binary package names as declared by `Package:` stanzas.
    pub binary_names: Vec<String>,
}

/// Parse and index a `debian/control` file.
///
/// `relative_path` is the path to record in the SCIP document (typically
/// `"debian/control"`). `version` may be the source package version from the
/// changelog and is embedded into emitted symbols.
pub fn index(text: &str, relative_path: &str, version: Option<&str>) -> ControlIndex {
    let parse = Control::parse(text);
    let control = parse.tree();
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();
    let mut external_binaries: HashSet<String> = HashSet::new();
    let mut build_profiles: HashSet<String> = HashSet::new();
    let mut source_name: Option<String> = None;
    let mut binary_names: Vec<String> = Vec::new();

    // Syntax-highlighting occurrences for the whole document.
    occurrences.extend(crate::scip::highlight::deb822(control.as_deb822(), &lines));

    if let Some(source) = control.source() {
        if let Some(name) = source.name() {
            source_name = Some(name.clone());
            let sym = symbols::source_package(&name, version);
            // Definition occurrence: the value of the `Source:` field.
            if let Some(entry) = source.as_deb822().get_entry("Source") {
                if let Some(tr) = entry.value_token_range() {
                    let range = (tr.start().into(), tr.end().into());
                    occurrences.push(occurrence(&lines, range, &sym, SymbolRole::Definition));
                }
            }
            symbols_info.push(SymbolInformation {
                symbol: sym.clone(),
                kind: scip::types::symbol_information::Kind::Package.into(),
                display_name: name.clone(),
                ..Default::default()
            });

            // References inside source-stanza relation fields.
            for field in SOURCE_RELATION_FIELDS {
                emit_relations(
                    source.as_deb822().get_entry(field),
                    text,
                    &lines,
                    &mut occurrences,
                    &mut external_binaries,
                    &mut build_profiles,
                );
            }

            // Maintainer / Uploaders identity references.
            for id in source
                .maintainer_identities()
                .into_iter()
                .chain(source.uploaders_identities())
            {
                occurrences.push(Occurrence {
                    range: lines.range(id.email_range.start().into(), id.email_range.end().into()),
                    symbol: symbols::identity(&id.email),
                    ..Default::default()
                });
            }
        }
    }

    for binary in control.binaries() {
        let Some(bname) = binary.name() else { continue };
        binary_names.push(bname.clone());
        let bin_sym =
            symbols::binary_package(source_name.as_deref().unwrap_or(&bname), version, &bname);
        // The whole `Package:` stanza is the enclosing scope of this binary.
        let stanza = binary.as_deb822().text_range();
        let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());
        if let Some(entry) = binary.as_deb822().get_entry("Package") {
            if let Some(tr) = entry.value_token_range() {
                occurrences.push(Occurrence {
                    range: lines.range(tr.start().into(), tr.end().into()),
                    symbol: bin_sym.clone(),
                    symbol_roles: SymbolRole::Definition as i32,
                    enclosing_range: enclosing_range.clone(),
                    ..Default::default()
                });
            }
        }
        // Each binary package references its source package, so "find
        // references" on the source surfaces the binaries it builds, and the
        // source package is its enclosing symbol.
        let (relationships, enclosing_symbol) = match source_name.as_deref() {
            Some(src) => (
                vec![symbols::rel_reference(symbols::source_package(
                    src, version,
                ))],
                symbols::source_package(src, version),
            ),
            None => (Vec::new(), String::new()),
        };
        symbols_info.push(SymbolInformation {
            symbol: bin_sym,
            kind: scip::types::symbol_information::Kind::Package.into(),
            relationships,
            enclosing_symbol,
            display_name: bname.clone(),
            ..Default::default()
        });

        for field in BINARY_RELATION_FIELDS {
            emit_relations(
                binary.as_deb822().get_entry(field),
                text,
                &lines,
                &mut occurrences,
                &mut external_binaries,
                &mut build_profiles,
            );
        }
    }

    ControlIndex {
        document: Document {
            language: "debcontrol".to_owned(),
            relative_path: relative_path.to_owned(),
            text: text.to_owned(),
            occurrences,
            symbols: symbols_info,
            position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                .into(),
            ..Default::default()
        },
        external_binaries,
        build_profiles,
        source_name,
        binary_names,
    }
}

fn emit_relations(
    entry: Option<deb822_lossless::Entry>,
    text: &str,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    external_binaries: &mut HashSet<String>,
    build_profiles: &mut HashSet<String>,
) {
    let Some(entry) = entry else { return };
    let Some(vr) = entry.value_range() else {
        return;
    };
    let value_start: u32 = u32::from(vr.start());
    let value_end: u32 = u32::from(vr.end());
    let value_text = &text[value_start as usize..value_end as usize];
    let (relations, _errors) =
        debian_control::lossless::relations::Relations::parse_relaxed(value_text, true);
    for rel_entry in relations.entries() {
        for relation in rel_entry.relations() {
            let Some(name) = relation.try_name() else {
                continue;
            };
            let Some(local) = relation.name_range() else {
                continue;
            };
            let abs_start = value_start + u32::from(local.start());
            let abs_end = value_start + u32::from(local.end());
            // Clamp defensively to the field's actual value range.
            if abs_start < value_start || abs_end > value_end {
                continue;
            }
            let sym = symbols::external_binary(&name);
            external_binaries.insert(name);
            occurrences.push(plain_occurrence(lines, (abs_start, abs_end), &sym));

            // Build-profile references inside this relation, e.g. `<!nocheck>`.
            for prof_range in relation.profile_ranges() {
                let s = value_start + u32::from(prof_range.start());
                let e = value_start + u32::from(prof_range.end());
                let local_s = (s - value_start) as usize;
                let local_e = (e - value_start) as usize;
                let profile_name = &value_text[local_s..local_e];
                build_profiles.insert(profile_name.to_owned());
                occurrences.push(Occurrence {
                    range: lines.range(s, e),
                    symbol: symbols::build_profile(profile_name),
                    ..Default::default()
                });
            }
        }
    }
}

fn occurrence(lines: &LineTable, range: (u32, u32), symbol: &str, role: SymbolRole) -> Occurrence {
    Occurrence {
        range: lines.range(range.0, range.1),
        symbol: symbol.to_owned(),
        symbol_roles: role as i32,
        ..Default::default()
    }
}

/// A reference to another package from a relation field. Marked as an `Import`
/// role with namespace highlighting.
fn plain_occurrence(lines: &LineTable, range: (u32, u32), symbol: &str) -> Occurrence {
    Occurrence {
        range: lines.range(range.0, range.1),
        symbol: symbol.to_owned(),
        symbol_roles: SymbolRole::Import as i32,
        syntax_kind: scip::types::SyntaxKind::IdentifierNamespace.into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Source: hello
Maintainer: Jelmer Vernooĳ <jelmer@debian.org>
Build-Depends: debhelper-compat (= 13), libfoo-dev

Package: hello
Architecture: any
Depends: ${shlibs:Depends}, libfoo1
Description: example
 long description
";

    #[test]
    fn indexes_source_and_binary() {
        let idx = index(SAMPLE, "debian/control", Some("2.10-3"));
        assert_eq!(idx.source_name.as_deref(), Some("hello"));
        assert_eq!(idx.binary_names, vec!["hello".to_owned()]);
        assert!(idx.external_binaries.contains("debhelper-compat"));
        assert!(idx.external_binaries.contains("libfoo-dev"));
        assert!(idx.external_binaries.contains("libfoo1"));
        // shlibs:Depends is a substvar, not a relation, so it should not appear.
        assert!(!idx.external_binaries.contains("shlibs:Depends"));
        assert!(!idx.external_binaries.contains("${shlibs:Depends}"));

        let occs = &idx.document.occurrences;
        // At least: source def, binary def, three relation refs.
        assert!(occs.len() >= 5, "occurrences = {occs:?}");

        // Source definition points at the right byte range.
        let src_def = occs
            .iter()
            .find(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .expect("expected at least one definition");
        assert_eq!(src_def.range, vec![0, 8, 0, 13]);

        // The binary package symbol references its source package.
        let bin_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == symbols::binary_package("hello", Some("2.10-3"), "hello"))
            .expect("binary symbol info");
        assert_eq!(bin_sym.relationships.len(), 1);
        assert_eq!(
            bin_sym.relationships[0].symbol,
            symbols::source_package("hello", Some("2.10-3"))
        );
        assert!(bin_sym.relationships[0].is_reference);
        // The binary symbol has a friendly display name and is enclosed by source.
        assert_eq!(bin_sym.display_name, "hello");
        assert_eq!(
            bin_sym.enclosing_symbol,
            symbols::source_package("hello", Some("2.10-3"))
        );

        // The binary definition occurrence carries an enclosing range spanning
        // the whole Package stanza (more than just the name).
        let bin_def = occs
            .iter()
            .find(|o| {
                o.symbol == symbols::binary_package("hello", Some("2.10-3"), "hello")
                    && (o.symbol_roles & SymbolRole::Definition as i32) != 0
            })
            .expect("binary definition occurrence");
        assert!(
            !bin_def.enclosing_range.is_empty(),
            "expected an enclosing range on the binary definition"
        );
    }
}
