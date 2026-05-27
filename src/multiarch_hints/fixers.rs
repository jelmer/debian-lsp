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
use std::str::FromStr;

use ::debian_workspace::action::ActionPlan;
use multiarch_hints::{
    detect_multiarch_hints, multiarch_hints_by_binary, Certainty, Hint, Severity,
};
use serde::{Deserialize, Serialize};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeDescription, Diagnostic,
    DiagnosticSeverity, NumberOrString, Range, Uri, WorkspaceEdit,
};

use crate::debian_workspace::translate::{
    is_action_translatable, plan_to_workspace_edit, plans_range, plans_range_in_file,
    plans_touch_file,
};
use crate::debian_workspace::workspace::LspDebianWorkspace;
use crate::workspace::{SourceFile, Workspace};
use crate::FileInfo;

/// Fix data carried on a published multiarch-hints [`Diagnostic`] via its
/// `data` field.
///
/// The detector pass already computes the [`ActionPlan`] that fixes each
/// hint. Serialising it here lets `code_action` reconstruct the quick fix
/// from the diagnostic the client echoes back, instead of re-running the
/// detector. A multiarch hint maps to exactly one plan, so this carries a
/// single `ActionPlan` rather than a list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MhDiagnosticData {
    /// The plan that fixes this hint.
    pub plan: ActionPlan,
}

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
        // The diagnostic describes the problem in the upstream's own
        // words ("foo could be MA: foreign"); the matching quickfix
        // action title carries the proposed change ("Add Multi-Arch:
        // foreign."). Fall back to the action label if the upstream
        // description is empty.
        let message = if change.hint.description.is_empty() {
            plan.label.clone()
        } else {
            change.hint.description.clone()
        };
        // Carry the fix plan on the diagnostic's `data` field so
        // `code_action` can reconstruct the quick fix without re-running
        // the detector. A serialisation failure is logged and the
        // diagnostic is still published — the fix just won't be available
        // until the file is reopened.
        let data = match serde_json::to_value(MhDiagnosticData { plan: plan.clone() }) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::error!(
                    "multiarch-hints: failed to serialise fix data for {}: {}",
                    change.hint.kind(),
                    e
                );
                None
            }
        };
        out.push(Diagnostic {
            range,
            severity: Some(hint_severity_to_lsp(change.hint.severity)),
            code: Some(NumberOrString::String(change.hint.kind().to_string())),
            code_description: Uri::from_str(&change.hint.link)
                .ok()
                .map(|href| CodeDescription { href }),
            source: Some("multiarch-hints".to_string()),
            message,
            data,
            ..Default::default()
        });
    }
    out
}

/// Reconstruct multiarch-hints quick fixes from the diagnostics the
/// client echoes back in a `textDocument/codeAction` request — without
/// re-running the detector.
///
/// Each multiarch-hints diagnostic carries its fix [`ActionPlan`] in the
/// LSP `data` field (see [`MhDiagnosticData`], attached by
/// [`run_diagnostics_for_uri`]). This function deserialises that data and
/// translates the plan into a [`CodeAction`]. Diagnostics without our
/// `data` (a different source, or published before this field existed)
/// are skipped.
pub fn actions_from_diagnostics(
    uri: &Uri,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let Some(base_path) = base_path_for_debian_file(uri) else {
        return Vec::new();
    };
    let Some(rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };
    // The detector only edits debian/control; a code action elsewhere
    // can carry no multiarch-hints fix.
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

    let mut actions = Vec::new();
    for lsp_diag in diagnostics {
        let Some(data) = &lsp_diag.data else {
            continue;
        };
        let parsed: MhDiagnosticData = match serde_json::from_value(data.clone()) {
            Ok(d) => d,
            // The diagnostic carries `data` we don't recognise — it
            // belongs to another source (e.g. lintian-brush). Skip it
            // silently rather than failing the whole request.
            Err(_) => continue,
        };
        if !parsed.plan.actions.iter().all(is_action_translatable) {
            continue;
        }
        let Some(edit) = plan_to_workspace_edit(&parsed.plan, &ws) else {
            continue;
        };
        actions.push(build_action_with_diagnostics(
            &parsed.plan.label,
            edit,
            vec![lsp_diag.clone()],
        ));
    }
    actions
}

/// Map a multiarch-hints `Severity` to its LSP counterpart.
///
/// `High` is a hard conflict (file-conflict, ma-foreign-library); the
/// fix is well-defined and worth surfacing prominently. `Normal` and
/// `Low` are suggestions, so they stay at the lower LSP levels.
fn hint_severity_to_lsp(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::High => DiagnosticSeverity::WARNING,
        Severity::Normal => DiagnosticSeverity::INFORMATION,
        Severity::Low => DiagnosticSeverity::HINT,
    }
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
        // The diagnostic message carries the upstream description.
        // The fix label ("Add Multi-Arch: foreign.") is the code action
        // title, not the diagnostic message.
        assert_eq!(diags[0].message, "foo could be MA: foreign");
        // code_description carries the wiki link so editors can render
        // a "More information" affordance.
        assert_eq!(
            diags[0]
                .code_description
                .as_ref()
                .map(|c| c.href.as_str().to_string()),
            Some("https://wiki.debian.org/MultiArch/Hints#ma-foreign".to_string())
        );
        // "normal" severity hints map to INFORMATION.
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::INFORMATION));
    }

    /// "high"-severity hints (file-conflict, ma-foreign-library) map
    /// to LSP WARNING — they describe an actual conflict, not a
    /// suggestion.
    #[test]
    fn high_severity_hint_maps_to_warning() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: same\nDescription: bar\n bar\n",
        );

        let yaml = format!(
            "format: multiarch-hints-1.0\nhints:\n- binary: foo\n  description: \"foo conflicts\"\n  link: https://wiki.debian.org/MultiArch/Hints#file-conflict\n  severity: high\n  source: src\n",
        );
        let hints = parse(&yaml);
        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &hints);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
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

    /// A published diagnostic carries its fix plan on the `data` field,
    /// and `actions_from_diagnostics` reconstructs the quick fix from that
    /// data alone — producing the same edit `run_fixers_for_uri` would,
    /// without re-running the detector.
    #[test]
    fn actions_from_diagnostics_reconstructs_fix_from_data() {
        let original = "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n";
        let (_tmp, workspace, open_files, control_uri) = setup_control(original);

        let hints = parse(&hint_yaml("foo", "ma-foreign", "foo could be MA: foreign"));

        // Publish phase: the detector run attaches the fix plan to `data`.
        let diags = run_diagnostics_for_uri(&control_uri, &workspace, &open_files, &hints);
        assert_eq!(diags.len(), 1);
        let diag = diags[0].clone();
        assert!(
            diag.data.is_some(),
            "published diagnostic should carry fix data"
        );

        // code_action phase: reconstruct the fix from the echoed-back
        // diagnostic, with no detector run.
        let actions = actions_from_diagnostics(
            &control_uri,
            &workspace,
            &open_files,
            std::slice::from_ref(&diag),
        );
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act) if act.title == "Add Multi-Arch: foreign." => {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected the ma-foreign fix reconstructed from diagnostic data");
        // The reconstructed action produces the same edit and links back
        // to the diagnostic it fixes.
        let edits = text_edits_for(
            action.edit.as_ref().expect("action carries an edit"),
            &control_uri,
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(
            apply_text_edit(original, &edits[0]),
            "Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: foreign\nDescription: bar\n bar\n",
        );
        assert!(
            action
                .diagnostics
                .as_ref()
                .is_some_and(|d| d.contains(&diag)),
            "reconstructed action should link to its diagnostic"
        );
    }

    /// A diagnostic without our `data` (a different source, or one
    /// published before the `data` field existed) yields no actions
    /// rather than failing the request.
    #[test]
    fn actions_from_diagnostics_skips_diagnostics_without_data() {
        let (_tmp, workspace, open_files, control_uri) = setup_control(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDescription: bar\n bar\n",
        );

        let bare_diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 2,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 10,
                },
            },
            code: Some(NumberOrString::String("some-other-tag".to_string())),
            message: "from another source".to_string(),
            ..Default::default()
        };
        let actions = actions_from_diagnostics(&control_uri, &workspace, &open_files, &[bare_diag]);
        assert!(
            actions.is_empty(),
            "a diagnostic without multiarch-hints data should produce no actions"
        );
    }
}
