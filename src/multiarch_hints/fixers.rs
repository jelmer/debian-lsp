//! Glue between debian-lsp's `code_action`/diagnostics handlers and the
//! `multiarch-hints` detector.
//!
//! The detector reads `debian/control` (via the shared
//! [`LspDebianWorkspace`] adapter) and produces one
//! `(Change, ActionPlan)` per applicable hint. We surface each plan as
//! one LSP `CodeAction` plus one LSP `Diagnostic` whose code is the hint
//! kind ("ma-foreign", "dep-any", …).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use multiarch_hints::{detect_multiarch_hints, multiarch_hints_by_binary, Certainty, Hint};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, DiagnosticSeverity,
    NumberOrString, Range, Uri, WorkspaceEdit,
};

use crate::debian_workspace::translate::{
    is_action_translatable, plan_to_workspace_edit, plans_range, plans_range_in_file,
    plans_touch_file,
};
use crate::debian_workspace::workspace::LspDebianWorkspace;
use crate::workspace::{SourceFile, Workspace};
use crate::FileInfo;

/// Run the multiarch-hints detector against the package rooted by `uri`
/// and return the resulting code actions.
///
/// `hints` is the parsed feed (see [`crate::multiarch_hints::hints::HintsStore`]).
/// Each `(Change, ActionPlan)` pair becomes one quickfix CodeAction
/// whose title is the change description ("Add Multi-Arch: foreign.", …).
pub fn run_fixers_for_uri(
    uri: &Uri,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
    diagnostics: &[Diagnostic],
    cursor_range: Option<Range>,
    hints: &[Hint],
) -> Vec<CodeActionOrCommand> {
    let Some(base_path) = base_path_for_debian_file(uri) else {
        return Vec::new();
    };
    let Some(rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };
    // The detector only edits debian/control. Skip work when the active
    // file is somewhere else — the LSP host already calls us per file.
    if rel != Path::new("debian/control") {
        return Vec::new();
    }

    let ws = LspDebianWorkspace::new(
        workspace,
        base_path,
        None,
        None,
        relevant_open_files(open_files),
    );

    let original = ws.current_text(&rel).unwrap_or_default();
    let original_idx = crate::position::LineIndex::new(&original);
    let original_src = crate::position::Source::new(&original, &original_idx);

    let by_binary = multiarch_hints_by_binary(hints);
    let detected = match detect_multiarch_hints(&ws, &by_binary, Certainty::Possible) {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!("multiarch-hints detector failed: {}", e);
            return Vec::new();
        }
    };

    let mut actions = Vec::new();
    for (change, plan) in detected {
        if !plan.actions.iter().all(is_action_translatable) {
            continue;
        }
        let plans = std::slice::from_ref(&plan);
        let Some(mh_range) = plans_range_in_file(plans, &ws, &rel, original_src) else {
            continue;
        };
        if let Some(cursor) = cursor_range {
            if !ranges_overlap_lsp(cursor, mh_range) {
                continue;
            }
        }

        let kind = change.hint.kind();
        let matching_lsp_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| diagnostic_matches_kind(d, kind) && ranges_overlap_lsp(d.range, mh_range))
            .cloned()
            .collect();

        if !diagnostics.is_empty() && matching_lsp_diags.is_empty() {
            continue;
        }

        let Some(edit) = plan_to_workspace_edit(&plan, &ws) else {
            continue;
        };
        actions.push(build_action_with_diagnostics(
            &plan.label,
            edit,
            matching_lsp_diags,
        ));
    }
    actions
}

/// Surface multiarch-hints detector results as LSP diagnostics on `uri`.
///
/// Only fires when `uri` points at `debian/control` — multiarch-hints
/// never touches any other file. Each diagnostic's `code` is the hint
/// kind ("ma-foreign", "dep-any", …) and `source` is "multiarch-hints".
pub fn run_diagnostics_for_uri(
    uri: &Uri,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
    hints: &[Hint],
) -> Vec<Diagnostic> {
    let Some(base_path) = base_path_for_debian_file(uri) else {
        return Vec::new();
    };
    let Some(rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };
    if rel != Path::new("debian/control") {
        return Vec::new();
    }

    let ws = LspDebianWorkspace::new(
        workspace,
        base_path,
        None,
        None,
        relevant_open_files(open_files),
    );

    let original = ws.current_text(&rel).unwrap_or_default();
    let original_idx = crate::position::LineIndex::new(&original);
    let original_src = crate::position::Source::new(&original, &original_idx);

    let by_binary = multiarch_hints_by_binary(hints);
    let detected = match detect_multiarch_hints(&ws, &by_binary, Certainty::Possible) {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!("multiarch-hints detector failed: {}", e);
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for (change, plan) in &detected {
        let plans = std::slice::from_ref(plan);
        if !plans_touch_file(plans, &rel) {
            continue;
        }
        let range = plans_range(plans, &ws, &rel, original_src);
        out.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::INFORMATION),
            code: Some(NumberOrString::String(change.hint.kind().to_string())),
            source: Some("multiarch-hints".to_string()),
            message: plan.label.clone(),
            ..Default::default()
        });
    }
    out
}

fn ranges_overlap_lsp(a: Range, b: Range) -> bool {
    a.start < b.end && b.start < a.end
}

fn build_action_with_diagnostics(
    title: &str,
    edit: WorkspaceEdit,
    diagnostics: Vec<Diagnostic>,
) -> CodeActionOrCommand {
    let action = CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(edit),
        diagnostics: if diagnostics.is_empty() {
            None
        } else {
            Some(diagnostics)
        },
        ..Default::default()
    };
    CodeActionOrCommand::CodeAction(action)
}

fn base_path_for_debian_file(uri: &Uri) -> Option<PathBuf> {
    let path = uri.to_file_path()?;
    let mut current = path.parent()?;
    loop {
        if current.file_name().and_then(|n| n.to_str()) == Some("debian") {
            return current.parent().map(Path::to_path_buf);
        }
        current = current.parent()?;
    }
}

fn package_relative_path(base_path: &Path, uri: &Uri) -> Option<PathBuf> {
    let abs = uri.to_file_path()?;
    abs.strip_prefix(base_path).ok().map(Path::to_path_buf)
}

fn relevant_open_files(open_files: &HashMap<Uri, FileInfo>) -> HashMap<Uri, SourceFile> {
    open_files
        .iter()
        .map(|(uri, info)| (uri.clone(), info.source_file))
        .collect()
}

fn diagnostic_matches_kind(diag: &Diagnostic, kind: &str) -> bool {
    matches!(&diag.code, Some(NumberOrString::String(s)) if s == kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileType;

    use multiarch_hints::{parse_multiarch_hints, Hint};
    use tower_lsp_server::ls_types::{
        DocumentChangeOperation, DocumentChanges, OneOf, Position, TextEdit,
    };

    fn hint_yaml(binary: &str, kind: &str, description: &str) -> String {
        // Quote the description: real upstream descriptions like "X
        // could be MA: foreign" contain a colon and would otherwise be
        // parsed as a YAML mapping.
        format!(
            "format: multiarch-hints-1.0\nhints:\n- binary: {binary}\n  description: \"{description}\"\n  link: https://wiki.debian.org/MultiArch/Hints#{kind}\n  severity: normal\n  source: src\n",
        )
    }

    fn parse(yaml: &str) -> Vec<Hint> {
        parse_multiarch_hints(yaml.as_bytes()).unwrap()
    }

    /// Builds a temp debian package with the given control file and a
    /// minimal changelog, opens the control file in a fresh
    /// `Workspace`, and returns everything the run_* helpers need.
    fn setup_control(control: &str) -> (tempfile::TempDir, Workspace, HashMap<Uri, FileInfo>, Uri) {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(debian.join("control"), control).unwrap();
        std::fs::write(
            debian.join("changelog"),
            "src (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let mut workspace = Workspace::new();
        let control_uri = Uri::from_file_path(debian.join("control")).unwrap();
        let source_file = workspace.update_file(control_uri.clone(), control.to_string());
        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );
        (tmp, workspace, open_files, control_uri)
    }

    /// Pull the `Vec<TextEdit>` for `uri` out of a `WorkspaceEdit`'s
    /// `document_changes` form.
    fn text_edits_for(edit: &WorkspaceEdit, uri: &Uri) -> Vec<TextEdit> {
        let Some(DocumentChanges::Operations(ops)) = edit.document_changes.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for op in ops {
            if let DocumentChangeOperation::Edit(text_doc_edit) = op {
                if &text_doc_edit.text_document.uri == uri {
                    for e in &text_doc_edit.edits {
                        if let OneOf::Left(te) = e {
                            out.push(te.clone());
                        }
                    }
                }
            }
        }
        out
    }

    /// Apply a single LSP `TextEdit` to `text` and return the result.
    /// Good enough for ASCII test inputs.
    fn apply_text_edit(text: &str, edit: &TextEdit) -> String {
        let start = lsp_pos_to_byte(text, edit.range.start);
        let end = lsp_pos_to_byte(text, edit.range.end);
        let mut out = String::with_capacity(text.len() + edit.new_text.len());
        out.push_str(&text[..start]);
        out.push_str(&edit.new_text);
        out.push_str(&text[end..]);
        out
    }

    fn lsp_pos_to_byte(text: &str, pos: Position) -> usize {
        let mut line = 0u32;
        let mut byte = 0usize;
        for (i, c) in text.char_indices() {
            if line == pos.line {
                let mut col = 0u32;
                let mut j = i;
                for cc in text[i..].chars() {
                    if cc == '\n' {
                        break;
                    }
                    if col == pos.character {
                        return j;
                    }
                    col += cc.encode_utf16(&mut [0u16; 2]).len() as u32;
                    j += cc.len_utf8();
                }
                return j;
            }
            byte = i + c.len_utf8();
            if c == '\n' {
                line += 1;
            }
        }
        byte
    }

    #[test]
    fn diagnostic_surfaces_for_ma_foreign() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n",
        );

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));
        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &hints);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("ma-foreign".to_string()))
        );
        assert_eq!(diags[0].source.as_deref(), Some("multiarch-hints"));
        assert_eq!(diags[0].message, "Add Multi-Arch: foreign.");
    }

    #[test]
    fn code_action_emits_workspace_edit_for_ma_foreign() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n",
        );

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));
        let actions = run_fixers_for_uri(&control_uri, &workspace, &open_files, &[], None, &hints);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign." => {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected 'Add Multi-Arch: foreign.' action");
        assert!(action.edit.is_some(), "action should carry a WorkspaceEdit");
    }

    #[test]
    fn no_hints_means_no_diagnostics() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n",
        );

        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &[]);
        assert!(diags.is_empty());
    }

    #[test]
    fn other_files_are_skipped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(debian.join("changelog"), "src (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n").unwrap();

        let mut workspace = Workspace::new();
        let changelog_uri = Uri::from_file_path(debian.join("changelog")).unwrap();
        let source_file = workspace.update_file(
            changelog_uri.clone(),
            std::fs::read_to_string(debian.join("changelog")).unwrap(),
        );
        let mut open_files = HashMap::new();
        open_files.insert(
            changelog_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Changelog,
            },
        );

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));
        let diags = run_diagnostics_for_uri(&changelog_uri, &workspace, &open_files, &hints);
        assert!(diags.is_empty());
        let actions =
            run_fixers_for_uri(&changelog_uri, &workspace, &open_files, &[], None, &hints);
        assert!(actions.is_empty());
    }

    /// When a hint targets the second of two binaries, the diagnostic
    /// range must anchor on *that* binary's paragraph — not the first
    /// one and not a whole-document fallback. Regression cover for the
    /// per-paragraph anchoring path in `plans_range`.
    #[test]
    fn diagnostic_range_anchors_on_correct_binary() {
        let control = "Source: src\n\
                       \n\
                       Package: foo\n\
                       Architecture: any\n\
                       Description: foo\n\
                       \n\
                       Package: bar\n\
                       Architecture: any\n\
                       Description: bar\n";
        let (_tmp, workspace, open_files, control_uri) = setup_control(control);

        let hints = parse(&hint_yaml("bar", "ma-foreign", "bar could be MA: foreign"));
        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &hints);
        assert_eq!(diags.len(), 1);
        // The `bar` paragraph starts on line 6 (0-based) in the control
        // above. A miss here would either point at line 2 (`foo`'s
        // paragraph) or at line 0 (whole-document fallback).
        assert_eq!(
            diags[0].range.start.line, 6,
            "diagnostic should anchor on the `bar` paragraph, got line {}",
            diags[0].range.start.line
        );
    }

    /// When the client passes context diagnostics, fixer actions must
    /// only fire for diagnostics whose `code` matches the hint kind.
    /// Mirrors lintian-brush's `run_fixers_filters_to_context_diagnostics`.
    #[test]
    fn fixer_filters_to_context_diagnostics() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n",
        );

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));

        // Empty context → action surfaces.
        let actions = run_fixers_for_uri(&control_uri, &workspace, &open_files, &[], None, &hints);
        assert!(
            actions.iter().any(|a| matches!(a,
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign."
            )),
            "expected the ma-foreign action with empty context.diagnostics"
        );

        // Context with an unrelated diagnostic → action suppressed.
        let unrelated = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            },
            code: Some(NumberOrString::String("some-other-tag".to_string())),
            message: "unrelated".to_string(),
            ..Default::default()
        };
        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[unrelated],
            None,
            &hints,
        );
        assert!(
            !actions.iter().any(|a| matches!(a,
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign."
            )),
            "ma-foreign action should be suppressed without a matching context diagnostic"
        );

        // Context with the matching diagnostic → action returns and is
        // linked to it. Use the same range the diagnostics pass emits so
        // the overlap check passes.
        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &hints);
        let matching = diags[0].clone();
        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[matching.clone()],
            None,
            &hints,
        );
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign." => {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected the ma-foreign action when a matching diagnostic is in context");
        assert!(
            action
                .diagnostics
                .as_ref()
                .is_some_and(|d| d.contains(&matching)),
            "code action should link back to the matching context diagnostic"
        );
    }

    /// End-to-end: apply the emitted TextEdit and check the resulting
    /// control file. Covers the deb822 SetField translator wiring for
    /// the ma-foreign fix.
    #[test]
    fn applied_edit_inserts_multi_arch_foreign() {
        let original = "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n";
        let (_tmp, workspace, open_files, control_uri) = setup_control(original);

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));
        let actions = run_fixers_for_uri(&control_uri, &workspace, &open_files, &[], None, &hints);
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign." => {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected 'Add Multi-Arch: foreign.' action");

        let edits = text_edits_for(action.edit.as_ref().unwrap(), &control_uri);
        assert_eq!(edits.len(), 1, "expected exactly one TextEdit");
        let applied = apply_text_edit(original, &edits[0]);
        assert_eq!(
            applied,
            "Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: foreign\nDescription: bar\n bar\n",
        );
    }
}
