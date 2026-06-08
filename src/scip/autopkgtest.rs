//! Index `debian/tests/control` (autopkgtest) into a SCIP document.
//!
//! Emits:
//! - One definition per test name in a `Tests:` field, scoped to the source
//!   package.
//! - One reference for every package in a `Depends:` field, pointing at the
//!   cross-package external-binary symbols shared with `debian/control`. The
//!   special tokens `@` and `@builddeps@` are skipped.
//! - One reference per `Restrictions:` token and per `Features:` token,
//!   pointing at cross-package symbols.
//!
//! The file is a deb822 document with one paragraph per test stanza.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use deb822_lossless::Deb822;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};
use std::collections::HashSet;

/// Indexed result for `debian/tests/control`.
pub struct AutopkgtestIndex {
    /// The SCIP document.
    pub document: Document,
    /// Names of external Debian binary packages referenced from `Depends:`.
    pub external_binaries: HashSet<String>,
    /// Restriction names referenced from `Restrictions:`.
    pub restrictions: HashSet<String>,
    /// Feature names referenced from `Features:`.
    pub features: HashSet<String>,
}

/// Parse and index a `debian/tests/control` file.
pub fn index(
    text: &str,
    relative_path: &str,
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
                let value_start = u32::from(vr.start());
                let value = &text[value_start as usize..u32::from(vr.end()) as usize];
                for (s, e, name) in iter_tokens(value, value_start) {
                    let sym = symbols::autopkgtest_test(source, version, &name);
                    occurrences.push(Occurrence {
                        range: lines.range(s, e),
                        symbol: sym.clone(),
                        symbol_roles: SymbolRole::Definition as i32 | SymbolRole::Test as i32,
                        syntax_kind: scip::types::SyntaxKind::IdentifierFunctionDefinition.into(),
                        ..Default::default()
                    });
                    symbols_info.push(SymbolInformation {
                        symbol: sym,
                        kind: scip::types::symbol_information::Kind::Method.into(),
                        display_name: name.clone(),
                        ..Default::default()
                    });
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
        external_binaries,
        restrictions,
        features,
    }
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
            let sym = symbols::external_binary(&name);
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
        let idx = index(SAMPLE, "debian/tests/control", "hello", Some("2.10-3"));

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
        // Test definitions carry the Test role alongside Definition.
        assert!(defs
            .iter()
            .all(|o| (o.symbol_roles & SymbolRole::Test as i32) != 0));

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
        let idx = index(SAMPLE, "debian/tests/control", "hello", Some("2.10-3"));

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
    fn iter_tokens_splits_on_space_and_comma() {
        let toks = iter_tokens("a, b  c,d", 0);
        let names: Vec<&str> = toks.iter().map(|t| t.2.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
        // First token range is correct.
        assert_eq!((toks[0].0, toks[0].1), (0, 1));
    }
}
