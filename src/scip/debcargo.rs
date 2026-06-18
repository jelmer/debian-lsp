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
                        index_identities(table, &lines, text, &mut occurrences);
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
                    // The root table spans the whole file, so it is not a useful
                    // enclosing scope for a top-level key.
                    None,
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
    // The table itself ([source] or [packages.NAME]) is the enclosing scope of
    // each of its keys.
    let enclosing = table.span();
    for (name, _) in table.iter() {
        emit_key(
            table,
            name,
            symbols::debcargo_key(source, version, scope, name),
            describe(name),
            enclosing.as_ref(),
            lines,
            occurrences,
            symbols_info,
        );
    }
}

/// Link the people named in `[source]` `maintainer` and `uploaders` to the same
/// cross-archive identity symbol as control Maintainer/Uploaders, so "find
/// references" on a person surfaces this package too. Both values are
/// `Name <email>` strings (uploaders being an array of them); we scan the
/// value's source span -- quotes and array brackets included -- for the emails.
fn index_identities(
    table: &Table,
    lines: &LineTable,
    text: &str,
    occurrences: &mut Vec<Occurrence>,
) {
    for key in ["maintainer", "uploaders"] {
        let Some(span) = table
            .get(key)
            .and_then(|i| i.as_value())
            .and_then(|v| v.span())
        else {
            continue;
        };
        for (email, rel_start, rel_end) in symbols::identity_emails(&text[span.clone()]) {
            occurrences.push(lines.identity_occurrence(
                email,
                (span.start + rel_start) as u32,
                (span.start + rel_end) as u32,
            ));
        }
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
            // The `[packages.NAME]` table is the name's enclosing scope.
            let enclosing = table.span();
            occurrences.push(definition(lines, span, &pkg_sym, enclosing.as_ref()));
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
#[allow(clippy::too_many_arguments)]
fn emit_key(
    table: &Table,
    name: &str,
    symbol: String,
    description: Option<&'static str>,
    enclosing: Option<&std::ops::Range<usize>>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    let Some(span) = table.key(name).and_then(|k| k.span()) else {
        return;
    };
    occurrences.push(definition(lines, span, &symbol, enclosing));
    symbols_info.push(SymbolInformation {
        symbol,
        kind: scip::types::symbol_information::Kind::Field.into(),
        display_name: name.to_owned(),
        documentation: description.map(str::to_owned).into_iter().collect(),
        ..Default::default()
    });
}

/// Build a highlighted definition occurrence covering a key's byte span,
/// optionally enclosed by its containing table's span.
fn definition(
    lines: &LineTable,
    span: std::ops::Range<usize>,
    symbol: &str,
    enclosing: Option<&std::ops::Range<usize>>,
) -> Occurrence {
    Occurrence {
        range: lines.range(span.start as u32, span.end as u32),
        symbol: symbol.to_owned(),
        symbol_roles: SymbolRole::Definition as i32,
        syntax_kind: SyntaxKind::IdentifierAttribute.into(),
        enclosing_range: enclosing
            .map(|e| lines.range(e.start as u32, e.end as u32))
            .unwrap_or_default(),
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
    fn table_keys_carry_enclosing_range_top_level_keys_do_not() {
        let idx = index(SAMPLE, "debian/debcargo.toml", "hello", Some("2.10-3"));
        let def = |scope: &str, key: &str| {
            let sym = symbols::debcargo_key("hello", Some("2.10-3"), scope, key);
            idx.document
                .occurrences
                .iter()
                .find(|o| o.symbol == sym)
                .unwrap_or_else(|| panic!("definition for {scope}/{key}"))
        };
        // Keys inside [source] and [packages.NAME] are enclosed by their table.
        assert!(!def("source", "homepage").enclosing_range.is_empty());
        assert!(!def("lib", "summary").enclosing_range.is_empty());
        // Top-level keys have no useful enclosing scope (the root table is the
        // whole file), so they carry none.
        assert!(def("", "overlay").enclosing_range.is_empty());
    }

    #[test]
    fn links_maintainer_and_uploaders_emails() {
        let text = "\
[source]
maintainer = \"Team <team@example.org>\"
uploaders = [\"Jane Doe <jane@example.org>\", \"John Roe <john@example.org>\"]
";
        let idx = index(text, "debian/debcargo.toml", "hello", None);

        for email in ["team@example.org", "jane@example.org", "john@example.org"] {
            let want = symbols::identity(email);
            assert!(
                idx.document.occurrences.iter().any(|o| o.symbol == want),
                "expected an identity occurrence for {email}"
            );
        }

        // The range covers just the email, not the surrounding `Name <...>` or
        // the TOML quotes. `maintainer = "Team <` is 20 cols on line 1.
        let team = symbols::identity("team@example.org");
        let occ = idx
            .document
            .occurrences
            .iter()
            .find(|o| o.symbol == team)
            .expect("maintainer identity occurrence");
        assert_eq!(occ.range, vec![1, 20, 1, 36]);
    }

    #[test]
    fn invalid_toml_emits_nothing() {
        let idx = index("not = = valid", "debian/debcargo.toml", "hello", None);
        assert!(idx.document.occurrences.is_empty());
    }
}
