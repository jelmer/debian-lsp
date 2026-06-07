//! Index `debian/debcargo.toml` into a SCIP document.
//!
//! Emits a definition occurrence and a documented symbol for each known key,
//! at the top level, in `[source]` and in each `[packages.NAME]` table. The
//! package names in `[packages.NAME]` headers are indexed too. Documentation
//! is reused from the LSP's debcargo field tables.

use crate::debcargo::fields;
use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole, SyntaxKind};
use toml_edit::{Document as TomlDocument, Table};

/// Indexed result for `debian/debcargo.toml`.
pub struct DebcargoIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index `debian/debcargo.toml`.
pub fn index(
    text: &str,
    relative_path: &str,
    source: &str,
    version: Option<&str>,
) -> DebcargoIndex {
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    // Parse with the immutable `Document` rather than `DocumentMut`: it retains
    // source spans, which `DocumentMut` strips on construction.
    if let Ok(doc) = TomlDocument::parse(text) {
        let root = doc.as_table();
        for (name, item) in root.iter() {
            match name {
                "source" => {
                    if let Some(table) = item.as_table() {
                        index_table(
                            table,
                            source,
                            version,
                            "source",
                            fields::source_key_description,
                            &lines,
                            &mut occurrences,
                            &mut symbols_info,
                        );
                    }
                }
                "packages" => {
                    if let Some(packages) = item.as_table() {
                        index_packages(
                            packages,
                            source,
                            version,
                            &lines,
                            &mut occurrences,
                            &mut symbols_info,
                        );
                    }
                }
                _ => emit_key(
                    root,
                    name,
                    symbols::debcargo_key(source, version, "", name),
                    fields::top_level_key_description(name),
                    &lines,
                    &mut occurrences,
                    &mut symbols_info,
                ),
            }
        }
    }

    DebcargoIndex {
        document: Document {
            language: "toml".to_owned(),
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

/// Index the keys of a single table (`[source]` or a `[packages.NAME]` table).
#[allow(clippy::too_many_arguments)]
fn index_table(
    table: &Table,
    source: &str,
    version: Option<&str>,
    scope: &str,
    describe: fn(&str) -> Option<&'static str>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    for (name, _) in table.iter() {
        emit_key(
            table,
            name,
            symbols::debcargo_key(source, version, scope, name),
            describe(name),
            lines,
            occurrences,
            symbols_info,
        );
    }
}

/// Index every `[packages.NAME]` sub-table: the package name itself and its keys.
fn index_packages(
    packages: &Table,
    source: &str,
    version: Option<&str>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    for (pkg_name, item) in packages.iter() {
        let Some(table) = item.as_table() else {
            continue;
        };
        // The package name in the `[packages.NAME]` header is a definition.
        let pkg_sym = symbols::debcargo_package(source, version, pkg_name);
        if let Some(span) = packages.key(pkg_name).and_then(|k| k.span()) {
            occurrences.push(definition(lines, span, &pkg_sym));
            symbols_info.push(SymbolInformation {
                symbol: pkg_sym,
                kind: scip::types::symbol_information::Kind::Namespace.into(),
                display_name: pkg_name.to_owned(),
                ..Default::default()
            });
        }
        index_table(
            table,
            source,
            version,
            pkg_name,
            fields::package_key_description,
            lines,
            occurrences,
            symbols_info,
        );
    }
}

/// Emit a definition occurrence and a documented symbol for a single key.
fn emit_key(
    table: &Table,
    name: &str,
    symbol: String,
    description: Option<&'static str>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    let Some(span) = table.key(name).and_then(|k| k.span()) else {
        return;
    };
    occurrences.push(definition(lines, span, &symbol));
    symbols_info.push(SymbolInformation {
        symbol,
        kind: scip::types::symbol_information::Kind::Field.into(),
        display_name: name.to_owned(),
        documentation: description.map(str::to_owned).into_iter().collect(),
        ..Default::default()
    });
}

/// Build a highlighted definition occurrence covering a key's byte span.
fn definition(lines: &LineTable, span: std::ops::Range<usize>, symbol: &str) -> Occurrence {
    Occurrence {
        range: lines.range(span.start as u32, span.end as u32),
        symbol: symbol.to_owned(),
        symbol_roles: SymbolRole::Definition as i32,
        syntax_kind: SyntaxKind::IdentifierAttribute.into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
overlay = \".\"
bin = false

[source]
homepage = \"https://example.org\"

[packages.lib]
summary = \"An example library\"
";

    #[test]
    fn indexes_keys_with_documentation() {
        let idx = index(SAMPLE, "debian/debcargo.toml", "hello", Some("2.10-3"));

        // Top-level, source and package keys each become a definition.
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .collect();
        // overlay, bin, homepage, [packages.lib] header, summary.
        assert_eq!(defs.len(), 5);

        let overlay = idx
            .document
            .symbols
            .iter()
            .find(|s| s.display_name == "overlay")
            .expect("overlay symbol");
        assert_eq!(
            overlay.documentation,
            vec![fields::top_level_key_description("overlay")
                .unwrap()
                .to_owned()]
        );

        let summary = idx
            .document
            .symbols
            .iter()
            .find(|s| s.display_name == "summary")
            .expect("summary symbol");
        assert_eq!(
            summary.documentation,
            vec![fields::package_key_description("summary")
                .unwrap()
                .to_owned()]
        );

        // The package name in the [packages.lib] header is indexed.
        assert!(idx
            .document
            .symbols
            .iter()
            .any(|s| s.symbol == symbols::debcargo_package("hello", Some("2.10-3"), "lib")));
    }

    #[test]
    fn invalid_toml_emits_nothing() {
        let idx = index("not = = valid", "debian/debcargo.toml", "hello", None);
        assert!(idx.document.occurrences.is_empty());
    }
}
