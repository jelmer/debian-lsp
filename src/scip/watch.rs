//! Index a `debian/watch` file into a SCIP document.
//!
//! Surfaces the upstream URL of each entry as a definition, so editor tooling
//! can show hover info and (eventually) navigate to upstream release pages.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use debian_watch::linebased::WatchFile;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};

/// Indexed result for a `debian/watch` file.
pub struct WatchIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index a `debian/watch` file.
pub fn index(text: &str, relative_path: &str, source: &str, version: Option<&str>) -> WatchIndex {
    let watch = WatchFile::from_str_relaxed(text);
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    // Syntax-highlighting occurrences for the whole file.
    occurrences.extend(crate::scip::highlight::watch(text, &lines));

    // Emit documented field/option-name symbols, reusing the same field table
    // as the LSP hover. v5 (deb822) uses title-case field names; v1-4 use
    // line-based option names (e.g. `uversionmangle`).
    match debian_watch::parse::Parse::parse(text).to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            crate::scip::fields::emit_field_symbols(
                wf.as_deb822(),
                &lines,
                &mut occurrences,
                &mut symbols_info,
                |field| symbols::watch_field(source, version, field),
                crate::watch::fields::field_description,
            );
        }
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            emit_linebased_option_symbols(
                &wf,
                &lines,
                source,
                version,
                &mut occurrences,
                &mut symbols_info,
            );
        }
    }

    for (i, entry) in watch.entries().enumerate() {
        let entry_sym =
            symbols::upstream_metadata_field(source, version, &format!("watch-entry-{}", i));
        if let Some(url_node) = entry.url_node() {
            let r = url_node.text_range();
            let s: u32 = r.start().into();
            let e: u32 = r.end().into();
            occurrences.push(Occurrence {
                range: lines.range(s, e),
                symbol: entry_sym.clone(),
                symbol_roles: SymbolRole::Definition as i32,
                syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
                ..Default::default()
            });
            symbols_info.push(SymbolInformation {
                symbol: entry_sym,
                kind: scip::types::symbol_information::Kind::Constant.into(),
                display_name: text[s as usize..e as usize].to_owned(),
                ..Default::default()
            });
        }
    }

    WatchIndex {
        document: Document {
            language: "debwatch".to_owned(),
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

/// Emit documented symbols for the keys of a v1-4 (line-based) watch file,
/// reusing the descriptions the LSP hover shows.
///
/// Each `KEY` token inside an `OPTION` node (e.g. `uversionmangle`), plus the
/// `version=` directive, becomes a definition occurrence pointing at a
/// documented `watch` field symbol. A repeated key is documented once but every
/// occurrence resolves to it.
fn emit_linebased_option_symbols(
    wf: &WatchFile,
    lines: &LineTable,
    source: &str,
    version: Option<&str>,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    use debian_watch::SyntaxKind;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for element in wf.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        if token.kind() != SyntaxKind::KEY {
            continue;
        }
        // The `version=` directive and option keys are documented; the `opts=`
        // key (under OPTS_LIST) is not.
        let (canonical, description): (&str, &str) = match token.parent().map(|p| p.kind()) {
            Some(SyntaxKind::OPTION) => {
                match crate::watch::fields::linebased_option_description(token.text()) {
                    Some(pair) => pair,
                    None => continue,
                }
            }
            Some(SyntaxKind::VERSION) => (
                token.text(),
                crate::watch::fields::version_directive_description(),
            ),
            _ => continue,
        };
        let sym = symbols::watch_field(source, version, canonical);
        let r = token.text_range();
        occurrences.push(Occurrence {
            range: lines.range(r.start().into(), r.end().into()),
            symbol: sym.clone(),
            symbol_roles: SymbolRole::Definition as i32,
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
version=4
opts=\"uversionmangle=s/-/./\" https://example.org/hello/ hello-(.+)\\.tar\\.gz
";

    #[test]
    fn indexes_url_definitions() {
        let idx = index(SAMPLE, "debian/watch", "hello", Some("2.10-3"));
        // URL definitions, excluding the documented option-name definitions.
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                (o.symbol_roles & SymbolRole::Definition as i32) != 0
                    && !o.symbol.contains("/watch/")
            })
            .collect();
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn highlights_v5_deb822_format() {
        let text = "Version: 5\n\nSource: https://example.org/hello/\nMatching-Pattern: hello-(.+)\\.tar\\.gz\n";
        let idx = index(text, "debian/watch", "hello", Some("2.10-3"));
        // v5 (deb822) watch files still get syntax-highlighting occurrences.
        let unspecified = scip::types::SyntaxKind::UnspecifiedSyntaxKind.into();
        assert!(
            idx.document
                .occurrences
                .iter()
                .any(|o| o.symbol.is_empty() && o.syntax_kind != unspecified),
            "expected highlight occurrences for a v5 watch file"
        );
    }

    #[test]
    fn v5_field_names_carry_documentation() {
        let text = "Version: 5\n\nSource: https://example.org/hello/\nMatching-Pattern: hello-(.+)\\.tar\\.gz\n";
        let idx = index(text, "debian/watch", "hello", Some("2.10-3"));

        let mp_sym = symbols::watch_field("hello", Some("2.10-3"), "Matching-Pattern");
        let mp = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == mp_sym)
            .expect("Matching-Pattern field symbol");
        assert_eq!(
            mp.documentation,
            vec![crate::watch::fields::field_description("Matching-Pattern")
                .unwrap()
                .1
                .to_owned()]
        );
    }

    #[test]
    fn v4_linebased_option_names_carry_documentation() {
        let idx = index(SAMPLE, "debian/watch", "hello", Some("2.10-3"));

        // The `uversionmangle` option is a documented symbol matching the LSP
        // hover description.
        let sym = symbols::watch_field("hello", Some("2.10-3"), "uversionmangle");
        let info = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == sym)
            .expect("uversionmangle option symbol");
        assert_eq!(
            info.documentation,
            vec![
                crate::watch::fields::linebased_option_description("uversionmangle")
                    .unwrap()
                    .1
                    .to_owned()
            ]
        );

        // Its key emits a Definition-role occurrence pointing at the symbol.
        assert!(idx
            .document
            .occurrences
            .iter()
            .any(|o| { o.symbol == sym && (o.symbol_roles & SymbolRole::Definition as i32) != 0 }));
    }

    #[test]
    fn v4_linebased_version_directive_carries_documentation() {
        let idx = index(SAMPLE, "debian/watch", "hello", Some("2.10-3"));

        let sym = symbols::watch_field("hello", Some("2.10-3"), "version");
        let info = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol == sym)
            .expect("version directive symbol");
        assert_eq!(
            info.documentation,
            vec![crate::watch::fields::version_directive_description().to_owned()]
        );
        assert!(idx
            .document
            .occurrences
            .iter()
            .any(|o| { o.symbol == sym && (o.symbol_roles & SymbolRole::Definition as i32) != 0 }));
    }

    #[test]
    fn v4_linebased_unknown_option_is_not_documented() {
        let text = "version=4\nopts=\"bogus=1\" https://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        let idx = index(text, "debian/watch", "hello", Some("2.10-3"));
        // The unrecognized `bogus` option is left to the highlighter, not a
        // symbol; only the `version` directive is documented here.
        let bogus = symbols::watch_field("hello", Some("2.10-3"), "bogus");
        assert!(idx.document.symbols.iter().all(|s| s.symbol != bogus));
        let watch_field_count = idx
            .document
            .symbols
            .iter()
            .filter(|s| s.symbol.contains("/watch/"))
            .count();
        assert_eq!(watch_field_count, 1);
    }
}
