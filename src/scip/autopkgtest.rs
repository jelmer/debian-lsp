//! Index `debian/tests/control` (autopkgtest) into a SCIP document.
//!
//! Emits:
//! - For each test name in a `Tests:` field, a reference to the test script's
//!   symbol when the script exists on disk (the definition lives in the script
//!   document, giving a cross-file jump), or a self-contained definition when
//!   it does not. Both are scoped to the source package.
//! - One document per referenced test script that exists, carrying a definition
//!   occurrence for the same symbol so "go to definition" lands on the script.
//! - One reference for every package in a `Depends:` field, pointing at the
//!   cross-package external-binary symbols shared with `debian/control`. The
//!   special tokens `@` and `@builddeps@` are skipped.
//! - One reference per `Restrictions:` token and per `Features:` token,
//!   pointing at cross-package symbols.
//!
//! The file is a deb822 document with one paragraph per test stanza.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use crate::tests::resolve::tests_directory;
use deb822_lossless::Deb822;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};
use std::collections::HashSet;
use std::path::Path;

/// Indexed result for `debian/tests/control`.
pub struct AutopkgtestIndex {
    /// The SCIP document for `debian/tests/control` itself.
    pub document: Document,
    /// One document per referenced test script that exists on disk.
    pub script_documents: Vec<Document>,
    /// Names of external Debian binary packages referenced from `Depends:`.
    pub external_binaries: HashSet<String>,
    /// Restriction names referenced from `Restrictions:`.
    pub restrictions: HashSet<String>,
    /// Feature names referenced from `Features:`.
    pub features: HashSet<String>,
}

/// Parse and index a `debian/tests/control` file.
///
/// `root` is the source-tree root (the directory containing `debian/`); test
/// scripts named in `Tests:` fields are resolved against it so the index can
/// emit cross-file links to the scripts that exist.
pub fn index(
    text: &str,
    relative_path: &str,
    root: &Path,
    source: &str,
    version: Option<&str>,
) -> AutopkgtestIndex {
    let deb822 = Deb822::parse(text).tree();
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();
    let mut external_binaries: HashSet<String> = HashSet::new();
    let mut restrictions: HashSet<String> = HashSet::new();
    let mut features: HashSet<String> = HashSet::new();
    let mut script_documents: Vec<Document> = Vec::new();

    // Syntax-highlighting occurrences for the whole document.
    occurrences.extend(crate::scip::highlight::deb822(&deb822, &lines));

    // Documented field-name symbols for every test stanza.
    crate::scip::fields::emit_field_symbols(
        &deb822,
        &lines,
        &mut occurrences,
        &mut symbols_info,
        |field| symbols::autopkgtest_field(source, version, field),
        |field| {
            crate::deb822::completion::field_description(crate::tests::fields::TESTS_FIELDS, field)
        },
    );

    for para in deb822.paragraphs() {
        if let Some(entry) = para.get_entry("Tests") {
            if let Some(vr) = entry.value_range() {
                let tests_dir = tests_directory(Some(&para), root);
                let value_start = u32::from(vr.start());
                let value = &text[value_start as usize..u32::from(vr.end()) as usize];
                for (s, e, name) in iter_tokens(value, value_start) {
                    let sym = symbols::autopkgtest_test(source, version, &name);
                    // When the script exists, the definition lives in the script
                    // document and this occurrence is a cross-file reference;
                    // otherwise it defines the symbol in place so it still
                    // resolves to itself.
                    let script_path = tests_dir.join(&name);
                    let resolved = std::fs::read_to_string(&script_path)
                        .ok()
                        .map(|script_text| (script_path, script_text));
                    let symbol_roles = if resolved.is_some() {
                        SymbolRole::Test as i32
                    } else {
                        SymbolRole::Definition as i32 | SymbolRole::Test as i32
                    };
                    occurrences.push(Occurrence {
                        range: lines.range(s, e),
                        symbol: sym.clone(),
                        symbol_roles,
                        syntax_kind: scip::types::SyntaxKind::IdentifierFunctionDefinition.into(),
                        ..Default::default()
                    });
                    if let Some((script_path, script_text)) = resolved {
                        if let Some(relative) = script_relative_path(root, &script_path) {
                            script_documents.push(index_test_script(
                                &script_text,
                                &relative,
                                &name,
                                sym.clone(),
                            ));
                        }
                    } else {
                        symbols_info.push(SymbolInformation {
                            symbol: sym,
                            kind: scip::types::symbol_information::Kind::Method.into(),
                            display_name: name.clone(),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        if let Some(entry) = para.get_entry("Depends") {
            emit_depends(
                &entry,
                text,
                &lines,
                &mut occurrences,
                &mut external_binaries,
            );
        }

        if let Some(entry) = para.get_entry("Restrictions") {
            if let Some(vr) = entry.value_range() {
                let value_start = u32::from(vr.start());
                let value = &text[value_start as usize..u32::from(vr.end()) as usize];
                for (s, e, name) in iter_tokens(value, value_start) {
                    occurrences.push(Occurrence {
                        range: lines.range(s, e),
                        symbol: symbols::autopkgtest_restriction(&name),
                        syntax_kind: scip::types::SyntaxKind::IdentifierAttribute.into(),
                        ..Default::default()
                    });
                    restrictions.insert(name);
                }
            }
        }

        if let Some(entry) = para.get_entry("Features") {
            if let Some(vr) = entry.value_range() {
                let value_start = u32::from(vr.start());
                let value = &text[value_start as usize..u32::from(vr.end()) as usize];
                for (s, e, name) in iter_tokens(value, value_start) {
                    occurrences.push(Occurrence {
                        range: lines.range(s, e),
                        symbol: symbols::autopkgtest_feature(&name),
                        syntax_kind: scip::types::SyntaxKind::IdentifierAttribute.into(),
                        ..Default::default()
                    });
                    features.insert(name);
                }
            }
        }
    }

    AutopkgtestIndex {
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
        script_documents,
        external_binaries,
        restrictions,
        features,
    }
}

/// Build a SCIP document for a test script, anchoring `sym`'s definition at its
/// first line so a `Tests:` reference jumps here.
fn index_test_script(text: &str, relative_path: &str, name: &str, sym: String) -> Document {
    let lines = LineTable::new(text);
    let occurrences = vec![Occurrence {
        range: lines.range(0, 0),
        symbol: sym.clone(),
        symbol_roles: SymbolRole::Definition as i32 | SymbolRole::Test as i32,
        ..Default::default()
    }];
    let symbols_info = vec![SymbolInformation {
        symbol: sym,
        kind: scip::types::symbol_information::Kind::Method.into(),
        display_name: name.to_owned(),
        ..Default::default()
    }];

    Document {
        language: "plain".to_owned(),
        relative_path: relative_path.to_owned(),
        text: text.to_owned(),
        occurrences,
        symbols: symbols_info,
        position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
        ..Default::default()
    }
}

/// The path of a test script relative to the source root, using forward slashes
/// for the SCIP `relative_path`. Returns `None` when the script lies outside the
/// root (e.g. an absolute or `..`-escaping `Tests-Directory`).
fn script_relative_path(root: &Path, script_path: &Path) -> Option<String> {
    let rel = script_path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::Normal(s) => parts.push(s.to_str()?.to_owned()),
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Emit external-binary references for an autopkgtest `Depends:` field.
///
/// Unlike a `debian/control` relation field, autopkgtest `Depends:` admits the
/// special tokens `@` (all binaries built by this source) and `@builddeps@`
/// (the source's build dependencies). Those are not package names, so they are
/// skipped.
fn emit_depends(
    entry: &deb822_lossless::Entry,
    text: &str,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    external_binaries: &mut HashSet<String>,
) {
    let Some(vr) = entry.value_range() else {
        return;
    };
    let value_start = u32::from(vr.start());
    let value_end = u32::from(vr.end());
    let value_text = &text[value_start as usize..value_end as usize];
    let (relations, _errors) =
        debian_control::lossless::relations::Relations::parse_relaxed(value_text, true);
    for rel_entry in relations.entries() {
        for relation in rel_entry.relations() {
            let Some(name) = relation.try_name() else {
                continue;
            };
            // Skip autopkgtest special tokens.
            if name == "@" || name == "@builddeps@" {
                continue;
            }
            let Some(local) = relation.name_range() else {
                continue;
            };
            let abs_start = value_start + u32::from(local.start());
            let abs_end = value_start + u32::from(local.end());
            if abs_start < value_start || abs_end > value_end {
                continue;
            }
            let sym = symbols::binary_package(&name);
            external_binaries.insert(name);
            occurrences.push(Occurrence {
                range: lines.range(abs_start, abs_end),
                symbol: sym,
                symbol_roles: SymbolRole::Import as i32,
                syntax_kind: scip::types::SyntaxKind::IdentifierNamespace.into(),
                ..Default::default()
            });
        }
    }
}

/// Iterate whitespace- or comma-separated tokens in a field value.
///
/// Yields `(absolute_start, absolute_end, token)` for each token, where the
/// offsets are relative to the original document given that `value` starts at
/// `base`.
fn iter_tokens(value: &str, base: u32) -> Vec<(u32, u32, String)> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_sep(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && !is_sep(bytes[i]) {
            i += 1;
        }
        out.push((
            base + start as u32,
            base + i as u32,
            value[start..i].to_owned(),
        ));
    }
    out
}

fn is_sep(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == b','
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const SAMPLE: &str = "\
Tests: smoke integration
Depends: @, python3-foo, @builddeps@
Restrictions: needs-root, allow-stderr

Test-Command: ./run-other
Depends: @
Features: test-name
";

    #[test]
    fn indexes_tests_depends_and_restrictions() {
        // No scripts on disk: the test names define their symbols in place.
        let dir = tempdir().unwrap();
        let idx = index(
            SAMPLE,
            "debian/tests/control",
            dir.path(),
            "hello",
            Some("2.10-3"),
        );

        // Two test definitions: smoke, integration. Filtered to test-role
        // definitions to exclude the documented field-name definitions.
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Test as i32) != 0)
            .collect();
        assert_eq!(defs.len(), 2, "defs: {defs:?}");
        assert!(defs.iter().any(|o| o.symbol.contains("smoke")));
        assert!(defs.iter().any(|o| o.symbol.contains("integration")));
        // Without scripts on disk the names carry Definition alongside Test.
        assert!(defs
            .iter()
            .all(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0));
        assert!(idx.script_documents.is_empty());

        // Depends references the real package only; @ and @builddeps@ skipped.
        assert!(idx.external_binaries.contains("python3-foo"));
        assert!(!idx.external_binaries.contains("@"));
        assert!(!idx.external_binaries.contains("@builddeps@"));

        // Restrictions become cross-package references.
        let restr: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| o.symbol.contains("autopkgtest-restriction"))
            .collect();
        assert_eq!(restr.len(), 2, "restrictions: {restr:?}");

        // Features become cross-package references.
        let feat: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| o.symbol.contains("autopkgtest-feature"))
            .collect();
        assert_eq!(feat.len(), 1, "features: {feat:?}");

        // The referenced restriction and feature names are reported for the
        // top-level indexer to emit as documented external symbols.
        assert!(idx.restrictions.contains("needs-root"));
        assert!(idx.restrictions.contains("allow-stderr"));
        assert!(idx.features.contains("test-name"));
    }

    #[test]
    fn field_names_carry_documentation() {
        let dir = tempdir().unwrap();
        let idx = index(
            SAMPLE,
            "debian/tests/control",
            dir.path(),
            "hello",
            Some("2.10-3"),
        );

        let restr_sym = symbols::autopkgtest_field("hello", Some("2.10-3"), "Restrictions");
        let restr = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == restr_sym)
            .expect("Restrictions field symbol");
        assert_eq!(
            restr.documentation,
            vec![crate::deb822::completion::field_description(
                crate::tests::fields::TESTS_FIELDS,
                "Restrictions"
            )
            .unwrap()
            .1
            .to_owned()]
        );

        // The field key emits a Definition occurrence distinct from the
        // test-name definitions (which also carry the Test role).
        let key_def = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == restr_sym)
            .expect("Restrictions field occurrence");
        assert_ne!(key_def.symbol_roles & SymbolRole::Definition as i32, 0);
        assert_eq!(key_def.symbol_roles & SymbolRole::Test as i32, 0);
    }

    #[test]
    fn existing_script_yields_cross_file_definition() {
        let dir = tempdir().unwrap();
        let tests = dir.path().join("debian").join("tests");
        std::fs::create_dir_all(&tests).unwrap();
        std::fs::write(tests.join("smoke"), "#!/bin/sh\necho ok\n").unwrap();

        let text = "Tests: smoke integration\n";
        let idx = index(
            text,
            "debian/tests/control",
            dir.path(),
            "hello",
            Some("2.10-3"),
        );

        let smoke_sym = symbols::autopkgtest_test("hello", Some("2.10-3"), "smoke");

        // The control occurrence for "smoke" is a reference (no Definition role),
        // since the definition now lives in the script document.
        let smoke_ref = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == smoke_sym)
            .expect("smoke occurrence in control");
        assert_eq!(smoke_ref.symbol_roles & SymbolRole::Definition as i32, 0);
        assert_ne!(smoke_ref.symbol_roles & SymbolRole::Test as i32, 0);

        // "integration" has no script, so it still defines itself in place.
        let integration_sym = symbols::autopkgtest_test("hello", Some("2.10-3"), "integration");
        let integration_ref = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == integration_sym)
            .expect("integration occurrence in control");
        assert_ne!(
            integration_ref.symbol_roles & SymbolRole::Definition as i32,
            0
        );

        // A script document is emitted for smoke only, defining the symbol.
        assert_eq!(idx.script_documents.len(), 1);
        let script = &idx.script_documents[0];
        assert_eq!(script.relative_path, "debian/tests/smoke");
        assert_eq!(script.text, "#!/bin/sh\necho ok\n");
        let def = script
            .occurrences
            .iter()
            .find(|o| o.symbol == smoke_sym)
            .expect("smoke definition in script");
        assert_ne!(def.symbol_roles & SymbolRole::Definition as i32, 0);
        assert!(script
            .symbols
            .iter()
            .any(|s| s.symbol == smoke_sym && s.display_name == "smoke"));
    }

    #[test]
    fn resolves_script_via_tests_directory() {
        let dir = tempdir().unwrap();
        let custom = dir.path().join("t");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("smoke"), "#!/bin/sh\n").unwrap();

        let text = "Tests: smoke\nTests-Directory: t\n";
        let idx = index(
            text,
            "debian/tests/control",
            dir.path(),
            "hello",
            Some("2.10-3"),
        );

        assert_eq!(idx.script_documents.len(), 1);
        assert_eq!(idx.script_documents[0].relative_path, "t/smoke");
    }

    #[test]
    fn script_relative_path_rejects_escaping() {
        let dir = tempdir().unwrap();
        let outside = dir.path().parent().unwrap().join("elsewhere");
        assert_eq!(script_relative_path(dir.path(), &outside), None);
    }

    #[test]
    fn iter_tokens_splits_on_space_and_comma() {
        let toks = iter_tokens("a, b  c,d", 0);
        let names: Vec<&str> = toks.iter().map(|t| t.2.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
        // First token range is correct.
        assert_eq!((toks[0].0, toks[0].1), (0, 1));
    }
}
