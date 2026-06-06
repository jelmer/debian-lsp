//! Index `debian/rules` (a Makefile) into a SCIP document.
//!
//! Emits:
//! - One definition per Makefile target.
//! - One definition per variable assignment.
//! - One reference for every `$(VAR)` / `${VAR}` use.
//! - One reference for every `dh_*` invocation inside recipe lines, pointing
//!   at a cross-package `debhelper` command symbol.
//!
//! The Makefile parser ([`makefile_lossless`]) gives us structured access to
//! rules, variable assignments, and variable references with byte ranges, so
//! the bulk of this module is wiring those into SCIP shape.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use makefile_lossless::Makefile;
use rowan::ast::AstNode;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};

/// Indexed result for a `debian/rules` file.
pub struct RulesIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index a `debian/rules` file.
pub fn index(text: &str, relative_path: &str, source: &str, version: Option<&str>) -> RulesIndex {
    let (makefile, _errors) = Makefile::from_str_relaxed(text);
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    // Syntax-highlighting occurrences for the whole file.
    occurrences.extend(crate::scip::highlight::makefile(&makefile, &lines));

    // Target definitions: anchor each at its first occurrence inside the rule
    // header (the text before the `:` operator).
    for rule in makefile.rules() {
        let rule_range = rule.syntax().text_range();
        let rule_start: u32 = rule_range.start().into();
        // The text we'll scan for the target name is the part of the rule
        // before the first colon.
        let rule_text = &text[rule_range];
        let header_end = rule_text
            .find(':')
            .map(|i| i + rule_start as usize)
            .unwrap_or(rule_range.end().into());
        let header = &text[rule_start as usize..header_end];
        for target in rule.targets() {
            // Skip phony pattern-y artefacts that aren't useful as defs.
            if target.is_empty() || target.contains('%') || target.starts_with('.') {
                // Still emit, but as a non-Definition occurrence — pattern
                // rules and dot-targets are valid Makefile constructs that
                // we want navigation for, just not "definition" semantics.
                if let Some((s, e)) = locate_in(header, &target, rule_start) {
                    occurrences.push(Occurrence {
                        range: lines.range(s, e),
                        symbol: symbols::rules_target(source, version, &target),
                        ..Default::default()
                    });
                }
                continue;
            }
            let Some((s, e)) = locate_in(header, &target, rule_start) else {
                continue;
            };
            let sym = symbols::rules_target(source, version, &target);
            // The whole rule (header + recipe) is the target's enclosing scope.
            let enclosing_range = lines.range(rule_range.start().into(), rule_range.end().into());
            occurrences.push(Occurrence {
                range: lines.range(s, e),
                symbol: sym.clone(),
                symbol_roles: SymbolRole::Definition as i32,
                enclosing_range,
                ..Default::default()
            });
            // An override_dh_* / execute_*_dh_* target implements the
            // corresponding debhelper command, so "find implementations" on the
            // dh command surfaces the rules targets that hook into it.
            let relationships = match crate::rules::debhelper::command_for_target(&target) {
                Some(cmd) => vec![symbols::rel_implementation(symbols::debhelper_command(cmd))],
                None => Vec::new(),
            };
            symbols_info.push(SymbolInformation {
                symbol: sym,
                kind: scip::types::symbol_information::Kind::Method.into(),
                display_name: target.clone(),
                documentation: crate::rules::fields::target_description(&target)
                    .into_iter()
                    .collect(),
                relationships,
                ..Default::default()
            });
        }

        // `dh_*` calls and variable references inside recipe lines.
        for recipe in rule.recipe_nodes() {
            let r = recipe.syntax().text_range();
            let recipe_text = &text[r];
            let base: u32 = r.start().into();
            // dh_* invocations are a Debian-specific concept handled by the
            // rules::debhelper module rather than the generic Makefile parser.
            for cmd in crate::rules::debhelper::iter_dh_commands(recipe_text, base) {
                occurrences.push(Occurrence {
                    range: lines.range(cmd.start, cmd.end),
                    symbol: symbols::debhelper_command(&cmd.name),
                    symbol_roles: SymbolRole::Import as i32,
                    ..Default::default()
                });
            }
            // Variable references inside recipes come from the parser.
            for vref in recipe.variable_references() {
                let r = vref.text_range();
                occurrences.push(Occurrence {
                    range: lines.range(r.start().into(), r.end().into()),
                    symbol: symbols::rules_variable(source, version, vref.name()),
                    symbol_roles: SymbolRole::ReadAccess as i32,
                    ..Default::default()
                });
            }
        }
    }

    // Variable assignment definitions.
    for var in makefile.variable_definitions() {
        let Some(name) = var.name() else { continue };
        let var_range = var.syntax().text_range();
        let var_start: u32 = var_range.start().into();
        let var_text = &text[var_range];
        let Some((s, e)) = locate_in(var_text, &name, var_start) else {
            continue;
        };
        let sym = symbols::rules_variable(source, version, &name);
        occurrences.push(Occurrence {
            range: lines.range(s, e),
            symbol: sym.clone(),
            symbol_roles: SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32,
            ..Default::default()
        });
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Variable.into(),
            display_name: name.clone(),
            documentation: crate::rules::fields::variable_description(&name)
                .map(str::to_owned)
                .into_iter()
                .collect(),
            ..Default::default()
        });
    }

    // Variable references.
    for vref in makefile.variable_references() {
        let Some(name) = vref.name() else { continue };
        let r = vref.text_range();
        // The reference range covers the whole `$(NAME)` or `${NAME}` token;
        // narrow to just `NAME` for the symbol occurrence.
        let ref_text = &text[r];
        let token_start: u32 = r.start().into();
        let Some((s, e)) = locate_in(ref_text, &name, token_start) else {
            continue;
        };
        occurrences.push(Occurrence {
            range: lines.range(s, e),
            symbol: symbols::rules_variable(source, version, &name),
            symbol_roles: SymbolRole::ReadAccess as i32,
            ..Default::default()
        });
    }

    RulesIndex {
        document: Document {
            language: "makefile".to_owned(),
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

/// Locate `needle` inside `haystack` and return its `(start, end)` byte offsets
/// in the original document, given that `haystack` starts at `haystack_base`.
fn locate_in(haystack: &str, needle: &str, haystack_base: u32) -> Option<(u32, u32)> {
    let pos = haystack.find(needle)?;
    let start = haystack_base + pos as u32;
    Some((start, start + needle.len() as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#!/usr/bin/make -f
DPKG_EXPORT_BUILDFLAGS = 1
include /usr/share/dpkg/default.mk

%:
\tdh $@

override_dh_auto_test:
\tdh_auto_test --no-act
\techo $(DPKG_EXPORT_BUILDFLAGS)
";

    #[test]
    fn indexes_targets_variables_and_dh_calls() {
        let idx = index(SAMPLE, "debian/rules", "hello", Some("2.10-3"));
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .collect();
        // Expected definitions:
        // - one variable assignment (DPKG_EXPORT_BUILDFLAGS)
        // - one rule target (override_dh_auto_test)
        // The `%` rule and bare `dh` recipe lack a useful def.
        let var_defs: Vec<_> = defs
            .iter()
            .filter(|o| o.symbol.contains("DPKG_EXPORT_BUILDFLAGS"))
            .collect();
        assert_eq!(var_defs.len(), 1, "defs: {defs:?}");
        let target_defs: Vec<_> = defs
            .iter()
            .filter(|o| o.symbol.contains("override_dh_auto_test"))
            .collect();
        assert_eq!(target_defs.len(), 1);

        // dh_* references: at least dh_auto_test (the `dh` on its own isn't dh_*).
        let dh_refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| o.symbol.contains("debhelper") && o.symbol.contains("dh_auto_test"))
            .collect();
        assert!(!dh_refs.is_empty(), "expected dh_auto_test ref");
        // dh calls are imports.
        assert!(dh_refs
            .iter()
            .all(|o| (o.symbol_roles & SymbolRole::Import as i32) != 0));

        // Variable assignment definition carries WriteAccess.
        let var_def = idx
            .document
            .occurrences
            .iter()
            .find(|o| {
                (o.symbol_roles & SymbolRole::Definition as i32) != 0
                    && o.symbol.contains("DPKG_EXPORT_BUILDFLAGS")
            })
            .expect("variable definition");
        assert!((var_def.symbol_roles & SymbolRole::WriteAccess as i32) != 0);

        // Variable reference back to DPKG_EXPORT_BUILDFLAGS carries ReadAccess.
        let var_refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| {
                (o.symbol_roles & SymbolRole::Definition as i32) == 0
                    && o.symbol.contains("DPKG_EXPORT_BUILDFLAGS")
            })
            .collect();
        assert!(!var_refs.is_empty(), "expected DPKG_EXPORT_BUILDFLAGS ref");
        assert!(var_refs
            .iter()
            .all(|o| (o.symbol_roles & SymbolRole::ReadAccess as i32) != 0));

        // The override target carries synthesised hover documentation.
        let target_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol.contains("override_dh_auto_test"))
            .expect("target symbol info");
        assert_eq!(target_sym.documentation, vec!["Override dh_auto_test step"]);

        // The known variable carries its description.
        let var_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol.contains("DPKG_EXPORT_BUILDFLAGS"))
            .expect("variable symbol info");
        assert_eq!(
            var_sym.documentation,
            vec!["Export dpkg build flags to the environment"]
        );

        // The override target implements the dh_auto_test debhelper command.
        let target_sym = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol.contains("override_dh_auto_test"))
            .expect("target symbol info");
        assert_eq!(target_sym.relationships.len(), 1);
        assert_eq!(
            target_sym.relationships[0].symbol,
            symbols::debhelper_command("dh_auto_test")
        );
        assert!(target_sym.relationships[0].is_implementation);
    }
}
