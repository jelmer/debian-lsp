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
    let mut field_symbols_seen: HashSet<String> = HashSet::new();

    // Syntax-highlighting occurrences for the whole document.
    occurrences.extend(crate::scip::highlight::deb822(control.as_deb822(), &lines));

    // Clickable links for URL-bearing fields (Homepage, Vcs-Browser, ...) and
    // URLs embedded in prose fields (Description).
    let mut url_symbols_seen: HashSet<String> = HashSet::new();
    crate::scip::links::emit_deb822(
        control.as_deb822(),
        crate::control::fields::CONTROL_FIELDS,
        text,
        &lines,
        &mut occurrences,
        &mut symbols_info,
        &mut url_symbols_seen,
    );

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

            // Documented field-name symbols for the source stanza.
            crate::scip::fields::emit_paragraph_field_symbols(
                source.as_deb822(),
                &lines,
                &mut occurrences,
                &mut symbols_info,
                &mut field_symbols_seen,
                |field| symbols::source_field(&name, version, field),
                control_field_description,
            );

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
                    syntax_kind: scip::types::SyntaxKind::IdentifierConstant.into(),
                    ..Default::default()
                });
            }
        }
    }

    for binary in control.binaries() {
        let Some(bname) = binary.name() else { continue };
        binary_names.push(bname.clone());
        // A binary package is named by the same symbol everywhere -- here at its
        // `Package:` stanza (its definition) and in every other package's relation
        // fields that reference it. So a `Depends: foo` elsewhere resolves to this
        // `Package: foo` line, the same way a `Provides:` entry does (see
        // `emit_provides`).
        let bin_sym = symbols::binary_package(&bname);
        // The whole `Package:` stanza is the enclosing scope of this binary.
        let stanza = binary.as_deb822().text_range();
        let enclosing_range = lines.range(stanza.start().into(), stanza.end().into());
        if let Some(entry) = binary.as_deb822().get_entry("Package") {
            if let Some(tr) = entry.value_token_range() {
                occurrences.push(Occurrence {
                    range: lines.range(tr.start().into(), tr.end().into()),
                    symbol: bin_sym.clone(),
                    symbol_roles: SymbolRole::Definition as i32,
                    syntax_kind: scip::types::SyntaxKind::IdentifierNamespace.into(),
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

        // Documented field-name symbols for this binary stanza.
        let field_source = source_name.clone().unwrap_or_else(|| bname.clone());
        crate::scip::fields::emit_paragraph_field_symbols(
            binary.as_deb822(),
            &lines,
            &mut occurrences,
            &mut symbols_info,
            &mut field_symbols_seen,
            |field| symbols::binary_field(&field_source, version, &bname, field),
            control_field_description,
        );

        for field in BINARY_RELATION_FIELDS {
            // `Provides` declares the (often versioned, virtual) binary packages
            // this package provides, so index each as a *definition* of the
            // external-binary symbol. A dependent's relation field references the
            // same symbol, so the dependency edge resolves -- e.g. a build-dep on
            // `librust-foo-0.5+default-dev` resolves to the source that provides
            // it. The other relation fields stay plain references.
            if *field == "Provides" {
                emit_provides(
                    binary.as_deb822().get_entry(field),
                    text,
                    &lines,
                    source_name.as_deref(),
                    version,
                    &mut occurrences,
                    &mut symbols_info,
                );
            } else {
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
            // The relaxed parser turns a substvar inside a version constraint
            // (e.g. `(= ${binary:Version})`) into a phantom relation named after
            // the substvar's inner ident; skip those, they aren't packages.
            if is_substvar_artifact(value_text, &local) {
                continue;
            }
            let abs_start = value_start + u32::from(local.start());
            let abs_end = value_start + u32::from(local.end());
            // Clamp defensively to the field's actual value range.
            if abs_start < value_start || abs_end > value_end {
                continue;
            }
            let sym = symbols::binary_package(&name);
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
                    syntax_kind: scip::types::SyntaxKind::IdentifierAttribute.into(),
                    ..Default::default()
                });
            }
        }
    }
}

/// Index a binary stanza's `Provides:` field. Each provided package name is a
/// *definition* of its external-binary symbol, related to the source package, so
/// a dependent's relation field (which references the same symbol) resolves to
/// the package that provides it.
#[allow(clippy::too_many_arguments)]
fn emit_provides(
    entry: Option<deb822_lossless::Entry>,
    text: &str,
    lines: &LineTable,
    source_name: Option<&str>,
    version: Option<&str>,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
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

    let (relationships, enclosing_symbol) = match source_name {
        Some(src) => (
            vec![symbols::rel_reference(symbols::source_package(
                src, version,
            ))],
            symbols::source_package(src, version),
        ),
        None => (Vec::new(), String::new()),
    };

    for rel_entry in relations.entries() {
        for relation in rel_entry.relations() {
            let Some(name) = relation.try_name() else {
                continue;
            };
            let Some(local) = relation.name_range() else {
                continue;
            };
            if is_substvar_artifact(value_text, &local) {
                continue;
            }
            let abs_start = value_start + u32::from(local.start());
            let abs_end = value_start + u32::from(local.end());
            if abs_start < value_start || abs_end > value_end {
                continue;
            }
            let sym = symbols::binary_package(&name);
            occurrences.push(occurrence(
                lines,
                (abs_start, abs_end),
                &sym,
                SymbolRole::Definition,
            ));
            symbols_info.push(SymbolInformation {
                symbol: sym,
                kind: scip::types::symbol_information::Kind::Package.into(),
                relationships: relationships.clone(),
                enclosing_symbol: enclosing_symbol.clone(),
                display_name: name,
                ..Default::default()
            });
        }
    }
}

/// Whether a parsed relation's name range falls inside a `${...}` substvar in
/// `value_text`. The relaxed relation parser can emit a phantom relation for the
/// ident inside a substvar (notably `${binary:Version}` in a version
/// constraint); such a name is preceded, ignoring `{`, by a `$`.
fn is_substvar_artifact(value_text: &str, name_range: &rowan::TextRange) -> bool {
    let start = usize::from(name_range.start());
    let before = value_text[..start.min(value_text.len())].trim_end_matches(['{', ' ']);
    before.ends_with('$')
}

/// A package-name occurrence (source/binary definition or provides). Package
/// names are namespaces, so they get [`SyntaxKind::IdentifierNamespace`].
/// Look up a `debian/control` field's canonical name and description.
fn control_field_description(field: &str) -> Option<(&'static str, &'static str)> {
    crate::deb822::completion::field_description(crate::control::fields::CONTROL_FIELDS, field)
}

fn occurrence(lines: &LineTable, range: (u32, u32), symbol: &str, role: SymbolRole) -> Occurrence {
    Occurrence {
        range: lines.range(range.0, range.1),
        symbol: symbol.to_owned(),
        symbol_roles: role as i32,
        syntax_kind: scip::types::SyntaxKind::IdentifierNamespace.into(),
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
            .find(|s| s.symbol == symbols::binary_package("hello"))
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
                o.symbol == symbols::binary_package("hello")
                    && (o.symbol_roles & SymbolRole::Definition as i32) != 0
            })
            .expect("binary definition occurrence");
        assert!(
            !bin_def.enclosing_range.is_empty(),
            "expected an enclosing range on the binary definition"
        );
    }

    /// A `Package:` stanza defines a binary by the same symbol another package's
    /// relation field uses to reference it, so a `Depends:` cross-package jump
    /// resolves to the defining `Package:` line. Both sides go through
    /// `symbols::binary_package`, version-independently.
    #[test]
    fn package_stanza_and_depends_share_one_symbol() {
        // A package that build-depends on `libfoo1` and also ships a `libfoo1`
        // binary: the definition and the reference must be the same symbol.
        let control = "Source: foo\n\
            Build-Depends: libfoo1\n\n\
            Package: libfoo1\n\
            Architecture: any\n\
            Description: lib\n";
        let idx = index(control, "debian/control", Some("1.0-1"));
        let want = symbols::binary_package("libfoo1");

        let def = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == want && (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .expect("Package: libfoo1 defines the binary symbol");
        // The definition is the `Package:` value, not the Build-Depends mention.
        assert_eq!(def.range, vec![3, 9, 3, 16]);

        let reference = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == want && (o.symbol_roles & SymbolRole::Definition as i32) == 0)
            .expect("Build-Depends references the same binary symbol");
        assert_eq!(reference.range, vec![1, 15, 1, 22]);
    }

    #[test]
    fn occurrences_carry_semantic_syntax_kinds() {
        use scip::types::SyntaxKind;
        let idx = index(SAMPLE, "debian/control", Some("2.10-3"));
        let occs = &idx.document.occurrences;

        let has = |kind: SyntaxKind| occs.iter().any(|o| o.syntax_kind == kind.into());

        // Field names (the deb822 keys) highlight as attributes.
        assert!(
            has(SyntaxKind::IdentifierAttribute),
            "expected a field-name occurrence (IdentifierAttribute)"
        );
        // Package names (source/binary defs and dep references) are namespaces.
        assert!(
            has(SyntaxKind::IdentifierNamespace),
            "expected a package-name occurrence (IdentifierNamespace)"
        );
        // The source definition is a package name.
        let src_def = occs
            .iter()
            .find(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .expect("source definition");
        assert_eq!(
            src_def.syntax_kind,
            SyntaxKind::IdentifierNamespace.into(),
            "source definition should be a namespace"
        );
        // The maintainer identity is a constant.
        let identity = occs
            .iter()
            .find(|o| o.symbol == symbols::identity("jelmer@debian.org"))
            .expect("maintainer identity occurrence");
        assert_eq!(identity.syntax_kind, SyntaxKind::IdentifierConstant.into());
    }

    #[test]
    fn links_homepage_and_vcs_urls() {
        let text = "\
Source: hello
Homepage: https://example.org/hello
Vcs-Browser: https://salsa.debian.org/debian/hello
Vcs-Git: https://salsa.debian.org/debian/hello.git

Package: hello
Architecture: any
Description: example
";
        let idx = index(text, "debian/control", Some("2.10-3"));
        for (field, url) in [
            ("Homepage", "https://example.org/hello"),
            ("Vcs-Browser", "https://salsa.debian.org/debian/hello"),
            ("Vcs-Git", "https://salsa.debian.org/debian/hello.git"),
        ] {
            let sym = symbols::web_url(url);
            let occ = idx
                .document
                .occurrences
                .iter()
                .find(|o| o.symbol == sym)
                .unwrap_or_else(|| panic!("no link occurrence for {url}"));
            assert_eq!(occ.symbol_roles & SymbolRole::Definition as i32, 0);
            let info = idx
                .document
                .symbols
                .iter()
                .find(|s| s.symbol == sym)
                .unwrap_or_else(|| panic!("no symbol info for {url}"));
            // The link doc names the originating control field.
            assert_eq!(
                info.documentation,
                vec![symbols::web_url_doc_labeled(field, url)]
            );
        }
    }

    const PROVIDES_SAMPLE: &str = "\
Source: rust-foo
Build-Depends: debhelper-compat (= 13)

Package: librust-foo-dev
Architecture: any
Provides:
 librust-foo+default-dev (= ${binary:Version}),
 librust-foo-0.5+default-dev (= ${binary:Version})
Depends: librust-bar-1+default-dev
";

    #[test]
    fn provides_are_indexed_as_definitions() {
        let idx = index(PROVIDES_SAMPLE, "debian/control", Some("0.5.16-1"));

        // Each provided (virtual) binary is a definition of its external-binary
        // symbol, so a dependent referencing it resolves to this package.
        for provided in ["librust-foo+default-dev", "librust-foo-0.5+default-dev"] {
            let sym = symbols::binary_package(provided);
            let def =
                idx.document.occurrences.iter().find(|o| {
                    o.symbol == sym && (o.symbol_roles & SymbolRole::Definition as i32) != 0
                });
            assert!(def.is_some(), "no definition occurrence for {provided}");

            let info = idx
                .document
                .symbols
                .iter()
                .find(|s| s.symbol == sym)
                .unwrap_or_else(|| panic!("no symbol info for {provided}"));
            // Related to the source package, so find-references on the source
            // surfaces what it provides.
            assert_eq!(
                info.relationships[0].symbol,
                symbols::source_package("rust-foo", Some("0.5.16-1"))
            );
        }

        // A Provides entry must not also be recorded as an external reference.
        assert!(!idx
            .external_binaries
            .contains("librust-foo-0.5+default-dev"));
        // Ordinary Depends relations are still plain references.
        assert!(idx.external_binaries.contains("librust-bar-1+default-dev"));

        // The `${binary:Version}` substvar inside the Provides version
        // constraint must not produce a phantom `binary` package symbol.
        let junk = symbols::binary_package("binary");
        assert!(
            !idx.document.occurrences.iter().any(|o| o.symbol == junk),
            "substvar produced a phantom `binary` symbol"
        );
    }

    #[test]
    fn field_names_carry_documentation() {
        let idx = index(SAMPLE, "debian/control", Some("2.10-3"));

        // The source-stanza `Build-Depends` field key is a documented symbol
        // carrying the same description the LSP hover shows.
        let bd_sym = symbols::source_field("hello", Some("2.10-3"), "Build-Depends");
        let bd = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == bd_sym)
            .expect("Build-Depends field symbol");
        assert_eq!(
            bd.documentation,
            vec![control_field_description("Build-Depends")
                .unwrap()
                .1
                .to_owned()]
        );

        // The binary-stanza `Architecture` field is scoped to its binary.
        let arch_sym = symbols::binary_field("hello", Some("2.10-3"), "hello", "Architecture");
        assert!(
            idx.document.symbols.iter().any(|s| s.symbol == arch_sym),
            "expected a documented Architecture field symbol on the binary stanza"
        );

        // A field key emits a Definition-role occurrence pointing at its symbol.
        assert!(idx.document.occurrences.iter().any(|o| {
            o.symbol == bd_sym && (o.symbol_roles & SymbolRole::Definition as i32) != 0
        }));
    }
}
