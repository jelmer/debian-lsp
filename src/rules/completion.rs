use makefile_lossless::Makefile;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::{RULES_TARGETS, RULES_VARIABLES};

/// Get completions for a debian/rules file at the given position.
pub fn get_completions(
    makefile: &Makefile,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let lines: Vec<&str> = source_text.lines().collect();
    let line = lines.get(position.line as usize).copied().unwrap_or("");

    // At column 0 on an empty line, offer target completions
    if position.character == 0 && line.trim().is_empty() {
        return get_target_completions(makefile);
    }

    // If the line starts with a tab, we're in a recipe — no completions for now
    if line.starts_with('\t') {
        return vec![];
    }

    // If the line looks like a variable assignment prefix, offer variable completions
    if position.character > 0 && !line.contains('=') && !line.contains(':') {
        return get_variable_completions();
    }

    vec![]
}

/// Generate target name completions, excluding targets already defined.
fn get_target_completions(makefile: &Makefile) -> Vec<CompletionItem> {
    let existing_targets: Vec<String> = makefile
        .rules()
        .flat_map(|r| r.targets().collect::<Vec<_>>())
        .collect();

    RULES_TARGETS
        .iter()
        .filter(|target| !existing_targets.iter().any(|t| t == target.name))
        .map(|target| {
            let detail = if target.required {
                format!("{} (required)", target.description)
            } else {
                target.description.to_string()
            };
            CompletionItem {
                label: target.name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail),
                insert_text: Some(format!("{}:\n\t", target.name)),
                ..Default::default()
            }
        })
        .collect()
}

/// Generate variable name completions.
fn get_variable_completions() -> Vec<CompletionItem> {
    RULES_VARIABLES
        .iter()
        .map(|var| CompletionItem {
            label: var.name.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(var.description.to_string()),
            insert_text: Some(format!("{} = ", var.name)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completions_empty_line() {
        let text = "#!/usr/bin/make -f\n\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let completions = get_completions(&makefile, text, Position::new(1, 0));
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.label == "clean"));
        assert!(completions.iter().any(|c| c.label == "build"));
    }

    #[test]
    fn test_completions_exclude_existing_targets() {
        let text = "clean:\n\trm -rf build\n\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let completions = get_completions(&makefile, text, Position::new(2, 0));
        // "clean" should not be offered since it already exists
        assert!(!completions.iter().any(|c| c.label == "clean"));
        // But "build" should still be offered
        assert!(completions.iter().any(|c| c.label == "build"));
    }

    #[test]
    fn test_completions_in_recipe() {
        let text = "clean:\n\t";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let completions = get_completions(&makefile, text, Position::new(1, 1));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_completions_target_insert_text() {
        let text = "\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let completions = get_completions(&makefile, text, Position::new(0, 0));
        let clean = completions.iter().find(|c| c.label == "clean").unwrap();
        assert_eq!(clean.insert_text.as_deref(), Some("clean:\n\t"));
        assert_eq!(clean.kind, Some(CompletionItemKind::FUNCTION));
    }
}
