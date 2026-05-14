//! Glue between debian-lsp's `code_action` handler and lintian-brush's
//! detector registry.
//!
//! Detectors live in `lintian_brush::workspace::iter_detector_registrations()`.
//! Each one takes our [`LspDebianWorkspace`] and returns
//! [`lintian_brush::diagnostic::Diagnostic`]s carrying serialisable
//! [`lintian_brush::diagnostic::Action`]s. We translate the actions into
//! LSP `TextEdit`s and surface each diagnostic as a `CodeAction`.

use super::translate::{
    diag_touches_file, diagnostic_range, diagnostic_range_in_file, is_action_translatable,
    parse_for_trigger_filtering_changelog, parse_for_trigger_filtering_deb822,
    parse_for_trigger_filtering_watch, parse_for_trigger_filtering_yaml, plan_to_workspace_edit,
};
use super::triggers::{
    phase_allow_net, phase_max_cost, triggers_match, ChangeContext, Deb822ChangeIndex,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ::lintian_brush::workspace::iter_detector_registrations;
use ::lintian_brush::{FixerPreferences, Version};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, NumberOrString, Range, Uri,
    WorkspaceEdit,
};

use crate::lintian_brush::workspace::LspDebianWorkspace;
use crate::workspace::{SourceFile, Workspace};
use crate::FileInfo;

pub use super::triggers::RunPhase;

/// Run every registered lintian-brush detector against the package
/// rooted by `uri` and return the resulting code actions. `uri` may
/// point at any file inside the package's `debian/` tree; the package
/// root is derived from it, and any code action whose plan we can
/// translate is surfaced (regardless of which file it edits).
///
/// Detectors that return `Err(_)` (including `NoChanges`) are silently
/// skipped, matching the existing wrap-and-sort / field-casing behaviour.
pub fn run_fixers_for_uri(
    uri: &Uri,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
    diagnostics: &[Diagnostic],
    cursor_range: Option<Range>,
    phase: RunPhase,
) -> Vec<CodeActionOrCommand> {
    let Some(base_path) = base_path_for_debian_file(uri) else {
        return Vec::new();
    };
    let Some(_rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };
    let (package, version) = match resolve_package_version(&base_path, workspace, open_files) {
        Some((p, v)) => (Some(p), Some(v)),
        None => (None, None),
    };

    let preferences = FixerPreferences {
        net_access: Some(phase_allow_net(phase)),
        ..Default::default()
    };

    let ws = LspDebianWorkspace::new(
        workspace,
        base_path.clone(),
        package,
        version,
        relevant_open_files(open_files),
    );

    let Some(rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };

    let original = ws.current_text(&rel).unwrap_or_default();
    let original_idx = crate::position::LineIndex::new(&original);
    let original_src = crate::position::Source::new(&original, &original_idx);

    // We don't apply trigger-based filtering for code-action
    // invocation: the user is asking "show me everything that needs
    // fixing in this package", not just on this file. Cost gating
    // still applies so a keystroke-mode invocation (rare for fixers in
    // practice) stays cheap.
    let max_cost = phase_max_cost(phase);
    let mut actions = Vec::new();
    for reg in iter_detector_registrations() {
        if reg.cost > max_cost {
            continue;
        }
        // Only instantiate the detector once we've decided to run it —
        // skipping ~150 Box<dyn Detector> allocations per call when
        // most detectors are gated out by cost.
        let detector = (reg.create)();
        let diags = match detector.detect(&ws, &preferences) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for diag in diags {
            // Filter out diagnostics that the user has explicitly silenced
            // via lintian overrides.
            if let Some(issue) = &diag.issue {
                use ::lintian_brush::workspace::FixerWorkspace as _;
                if !ws.should_fix(issue) {
                    continue;
                }
            }

            // Locate the specific range within the anchor file that this
            // diagnostic targets.  If no action in any plan targets the
            // anchor file, this diagnostic doesn't belong here and we
            // skip it — falling back to full_document_range would cause
            // the action to appear on every line of the file.
            let Some(lb_range) = diagnostic_range_in_file(&diag, &ws, &rel, original_src) else {
                continue;
            };

            // If a cursor range was provided, only show actions that overlap with it.
            // This prevents the "wrong paragraph" bug where all fixes for the whole
            // file are shown at every position.
            if let Some(cursor) = cursor_range {
                if !ranges_overlap_lsp(cursor, lb_range) {
                    continue;
                }
            }

            // Link to provided diagnostics that match the tag and range.
            // We use the actual tag from the issue, not reg.lintian_tags,
            // because some detectors emit tags not in their registration.
            let tag = diag.issue.as_ref().and_then(|i| i.tag.as_deref());
            let matching_lsp_diags: Vec<_> = diagnostics
                .iter()
                .filter(|d| {
                    let tag_matches = tag.map(|t| diagnostic_matches_tag(d, t)).unwrap_or(false);
                    // Use overlap for linking too, to be more robust than exact match.
                    tag_matches && ranges_overlap_lsp(d.range, lb_range)
                })
                .cloned()
                .collect();

            // Each detector may carry multiple alternative ActionPlans.
            // Offer all plans whose actions we can fully translate.
            for plan in &diag.plans {
                if !plan.actions.iter().all(is_action_translatable) {
                    continue;
                }
                let Some(edit) = plan_to_workspace_edit(plan, &ws) else {
                    continue;
                };
                actions.push(build_action_with_diagnostics(
                    &plan.label,
                    edit,
                    matching_lsp_diags.clone(),
                ));
            }

            // For any diagnostic with a LintianIssue, also offer a
            // "suppress with lintian override" action via the standard
            // plan translation path.
            if let Some(issue) = &diag.issue {
                if let Some(plan) = ::lintian_brush::diagnostic::override_action_plan(issue) {
                    if let Some(edit) = plan_to_workspace_edit(&plan, &ws) {
                        actions.push(build_action_with_diagnostics(
                            &plan.label,
                            edit,
                            matching_lsp_diags.clone(),
                        ));
                    }
                }
            }
        }
    }

    actions
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

/// Run every registered lintian-brush detector and surface the resulting
/// diagnostics as LSP [`Diagnostic`]s on `uri`. Only diagnostics whose
/// plan touches `uri` are surfaced — a control-only detector fires
/// silently when the user is editing `debian/copyright`.
///
/// Each `Diagnostic` from a detector becomes one LSP diagnostic with
/// `code` set to the lintian tag, `source` = "lintian-brush". The range
/// is derived from the action that targets `uri`, anchoring on the
/// specific field/paragraph/entry where possible and falling back to a
/// whole-document range otherwise.
pub fn run_diagnostics_for_uri(
    uri: &Uri,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
    phase: RunPhase,
    changed_ranges: Option<&[rowan::TextRange]>,
) -> Vec<Diagnostic> {
    let Some(base_path) = base_path_for_debian_file(uri) else {
        return Vec::new();
    };
    let Some(rel) = package_relative_path(&base_path, uri) else {
        return Vec::new();
    };
    let (package, version) = match resolve_package_version(&base_path, workspace, open_files) {
        Some((p, v)) => (Some(p), Some(v)),
        None => (None, None),
    };
    let preferences = FixerPreferences {
        net_access: Some(phase_allow_net(phase)),
        ..Default::default()
    };

    let ws = LspDebianWorkspace::new(
        workspace,
        base_path,
        package,
        version,
        relevant_open_files(open_files),
    );
    let original = ws.current_text(&rel).unwrap_or_default();
    let original_idx = crate::position::LineIndex::new(&original);
    let original_src = crate::position::Source::new(&original, &original_idx);
    let deb822_parse = parse_for_trigger_filtering_deb822(&ws, &rel);
    let changelog_parse = parse_for_trigger_filtering_changelog(&ws, &rel);
    let yaml_parse = parse_for_trigger_filtering_yaml(&ws, &rel);
    let watch_parse = parse_for_trigger_filtering_watch(&ws, &rel);
    let max_cost = phase_max_cost(phase);

    // Build the deb822 (paragraph_key, field) index once. With ~200
    // Trigger::Deb822Field across the registry, this turns 200×O(P+E)
    // tree walks per call into a single walk + 200 set lookups.
    let deb822_index = match (deb822_parse.as_ref(), changed_ranges) {
        (Some(deb822), Some(ranges)) => Some(Deb822ChangeIndex::build(deb822, ranges)),
        _ => None,
    };
    let ctx = ChangeContext {
        deb822: deb822_index,
        changelog: changelog_parse.as_ref(),
        yaml: yaml_parse.as_ref(),
        watch: watch_parse.as_ref(),
        changed_ranges,
    };

    let mut out = Vec::new();
    for reg in iter_detector_registrations() {
        if reg.cost > max_cost {
            continue;
        }
        if !triggers_match(reg.triggers, &rel, &ctx) {
            continue;
        }
        // Only instantiate the detector once we've decided to run it.
        let detector = (reg.create)();
        let diags = match detector.detect(&ws, &preferences) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for diag in diags {
            // Honour lintian overrides — same filter used in
            // `run_fixers_for_uri`. A user who suppressed the tag
            // shouldn't see a squiggle for it.
            if let Some(issue) = &diag.issue {
                use ::lintian_brush::workspace::FixerWorkspace as _;
                if !ws.should_fix(issue) {
                    continue;
                }
            }
            // Only surface diagnostics whose plan touches the current
            // file. A control-only detector firing while the user edits
            // copyright would otherwise produce a useless whole-document
            // squiggle on the wrong file.
            if !diag_touches_file(&diag, &rel) {
                continue;
            }
            let Some(tag) = diag.issue.as_ref().and_then(|i| i.tag.clone()) else {
                continue;
            };
            let range = diagnostic_range(&diag, &ws, &rel, original_src);
            out.push(Diagnostic {
                range,
                severity: Some(tower_lsp_server::ls_types::DiagnosticSeverity::INFORMATION),
                code: Some(NumberOrString::String(tag)),
                source: Some("lintian-brush".to_string()),
                // Prefer the plan's imperative label ("Fix X.") over the
                // diagnostic's explanatory message ("X is wrong."). The
                // squiggle hover then reads as a fix the user can take.
                message: diag
                    .plans
                    .first()
                    .map(|p| p.label.clone())
                    .unwrap_or_else(|| diag.message.clone()),
                ..Default::default()
            });
        }
    }
    out
}

/// Look up the debian package root for any URI inside a `debian/`
/// directory. Walks up until a parent named `debian` is found and
/// returns its parent. Works for `debian/control`, `debian/copyright`,
/// `debian/upstream/metadata`, `debian/patches/foo.patch`, etc.
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

/// Compute the package-relative path (e.g. `debian/copyright`) for a URI
/// inside a package's `debian/` tree.
fn package_relative_path(base_path: &Path, uri: &Uri) -> Option<PathBuf> {
    let abs = uri.to_file_path()?;
    abs.strip_prefix(base_path).ok().map(Path::to_path_buf)
}

fn resolve_package_version(
    base_path: &Path,
    workspace: &Workspace,
    open_files: &HashMap<Uri, FileInfo>,
) -> Option<(String, Version)> {
    let changelog_path = base_path.join("debian/changelog");
    let changelog_uri = Uri::from_file_path(&changelog_path)?;
    // Use the salsa-cached parse when the changelog is open in the
    // editor — otherwise this would re-parse the entire changelog on
    // every keystroke in any debian/* file, since the lintian-brush
    // diagnostic and code-action paths both call it. Fall back to a
    // disk read + one-shot parse only when the file isn't tracked.
    let parsed = if let Some(info) = open_files.get(&changelog_uri) {
        workspace.get_parsed_changelog(info.source_file).tree()
    } else {
        let text = std::fs::read_to_string(&changelog_path).ok()?;
        debian_changelog::ChangeLog::parse_relaxed(&text)
    };
    let entry = parsed.iter().next()?;
    let package = entry.package()?;
    let version = entry.version()?;
    Some((package, version))
}

fn relevant_open_files(open_files: &HashMap<Uri, FileInfo>) -> HashMap<Uri, SourceFile> {
    open_files
        .iter()
        .map(|(uri, info)| (uri.clone(), info.source_file))
        .collect()
}

fn diagnostic_matches_tag(diag: &Diagnostic, tag: &str) -> bool {
    matches!(&diag.code, Some(NumberOrString::String(s)) if s == tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileType;

    use ::lintian_brush::diagnostic::{
        Action, ChangelogAction, Deb822Action, FilesystemAction, ParagraphSelector, WatchAction,
        YamlAction,
    };
    use debian_changelog::ChangeLog;
    use debian_control::lossless::Control;
    use debian_copyright::lossless::Copyright;
    use tower_lsp_server::ls_types::{
        DocumentChangeOperation, DocumentChanges, OneOf, Position, ResourceOp, TextEdit,
    };

    use super::super::changelog_edits::changelog_action_to_text_edits;
    use super::super::deb822_edits::{
        append_paragraph_edits, copyright_action_to_text_edits, drop_relation_edits,
        drop_substvar_edits, ensure_substvar_edits, normalize_field_spacing_edits,
        remove_paragraph_edits,
    };
    use super::super::format_edits::{
        substitute_edits, watch_action_to_text_edits, yaml_action_to_text_edits,
    };

    /// Build a `Source` over `text` for tests. Stores the `LineIndex` in
    /// the caller's scope via a `let` binding pattern: `let idx =
    /// idx_of(text); let src = src_of(text, &idx);`.
    fn idx_of(text: &str) -> crate::position::LineIndex {
        crate::position::LineIndex::new(text)
    }
    fn src_of<'a>(
        text: &'a str,
        idx: &'a crate::position::LineIndex,
    ) -> crate::position::Source<'a> {
        crate::position::Source::new(text, idx)
    }

    /// Smoke test: a `debian/control` with `Maintainer: QA Folks
    /// <packages@qa.debian.org>` should produce one code action from the
    /// `wrong-debian-qa-group-name` detector, with a TextEdit that
    /// rewrites the maintainer line.
    #[test]
    fn qa_group_fix_surfaces_as_code_action() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        std::fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let mut workspace = Workspace::new();
        let control_path = debian.join("control");
        let control_uri = Uri::from_file_path(&control_path).unwrap();
        let source_file = workspace.update_file(
            control_uri.clone(),
            std::fs::read_to_string(&control_path).unwrap(),
        );

        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );

        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[],
            None,
            RunPhase::Explicit,
        );

        let qa_action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act)
                    if act.title == "Fix Debian QA group name." =>
                {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected a 'Fix Debian QA group name.' action");
        let edit = qa_action
            .edit
            .as_ref()
            .expect("action carries a WorkspaceEdit");
        let edits = first_text_edits_for(edit, &control_uri).expect("edit targets the control URI");
        // Structural edit: the value range only, not the whole line/file.
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].new_text, "Debian QA Group <packages@qa.debian.org>",
            "edit should replace just the maintainer value; got new_text: {:?}",
            edits[0].new_text
        );
        // Apply the edit and check the resulting text is what we expect.
        let original_src = std::fs::read_to_string(&control_path).unwrap();
        let applied = apply_text_edit_to_string(&original_src, &edits[0]);
        assert_eq!(
            applied,
            "Source: foo\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: foo\nDescription: bar\n bar\n",
        );
    }

    /// A diagnostic with a LintianIssue should produce a "suppress" override
    /// action in addition to the fix action.
    #[test]
    fn suppress_override_action_is_offered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        std::fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let mut workspace = Workspace::new();
        let control_path = debian.join("control");
        let control_uri = Uri::from_file_path(&control_path).unwrap();
        let source_file = workspace.update_file(
            control_uri.clone(),
            std::fs::read_to_string(&control_path).unwrap(),
        );
        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );

        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[],
            None,
            RunPhase::Explicit,
        );

        let suppress = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act)
                    if act.title.starts_with("Add lintian override for ") =>
                {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected an 'Add lintian override for ...' action");

        // The edit should target the overrides file.
        let overrides_uri =
            Uri::from_file_path(tmp.path().join("debian/source/lintian-overrides")).unwrap();
        let edits = first_text_edits_for(suppress.edit.as_ref().unwrap(), &overrides_uri)
            .expect("suppress action must target the overrides file");
        assert_eq!(edits.len(), 1);
        // The inserted text should contain the lintian tag.
        assert_eq!(
            edits[0].new_text,
            "faulty-debian-qa-group-phrase Maintainer QA Folks -> Debian QA Group\n"
        );
    }

    /// Pull the `Vec<TextEdit>` for `uri` out of a `WorkspaceEdit`'s
    /// `document_changes` form. Returns `None` if there's no
    /// `TextDocumentEdit` for that URI.
    fn first_text_edits_for(edit: &WorkspaceEdit, uri: &Uri) -> Option<Vec<TextEdit>> {
        let DocumentChanges::Operations(ops) = edit.document_changes.as_ref()? else {
            return None;
        };
        for op in ops {
            if let DocumentChangeOperation::Edit(text_doc_edit) = op {
                if &text_doc_edit.text_document.uri == uri {
                    let mut out = Vec::new();
                    for e in &text_doc_edit.edits {
                        if let OneOf::Left(te) = e {
                            out.push(te.clone());
                        }
                    }
                    return Some(out);
                }
            }
        }
        None
    }

    /// Diagnostics smoke test: a `Maintainer: QA Folks <packages@qa.debian.org>`
    /// in `debian/control` should produce one LSP `Diagnostic` whose code
    /// matches the lintian tag and whose range covers the maintainer entry.
    #[test]
    fn qa_group_surfaces_as_diagnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        std::fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let mut workspace = Workspace::new();
        let control_uri = Uri::from_file_path(debian.join("control")).unwrap();
        let source_file = workspace.update_file(
            control_uri.clone(),
            std::fs::read_to_string(debian.join("control")).unwrap(),
        );

        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );

        let diagnostics = run_diagnostics_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            RunPhase::Explicit,
            None,
        );
        let qa = diagnostics
            .iter()
            .find(|d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "faulty-debian-qa-group-phrase"))
            .expect("expected a faulty-debian-qa-group-phrase diagnostic");
        assert_eq!(qa.message, "Fix Debian QA group name.");
        assert_eq!(qa.source.as_deref(), Some("lintian-brush"));
        // Range covers the Maintainer entry, which is on line 1 (0-based)
        // of the control file.
        assert_eq!(qa.range.start.line, 1);
    }

    /// run_fixers_for_uri only emits quickfix actions that match a diagnostic
    /// in context.diagnostics when that list is non-empty.
    #[test]
    fn run_fixers_filters_to_context_diagnostics() {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: QA Folks <packages@qa.debian.org>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        std::fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let mut workspace = Workspace::new();
        let control_uri = Uri::from_file_path(debian.join("control")).unwrap();
        let source_file = workspace.update_file(
            control_uri.clone(),
            std::fs::read_to_string(debian.join("control")).unwrap(),
        );
        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );

        // With no context diagnostics: action is returned.
        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[],
            None,
            RunPhase::Explicit,
        );
        assert!(
            actions.iter().any(|a| matches!(a,
                CodeActionOrCommand::CodeAction(act) if act.title == "Fix Debian QA group name."
            )),
            "expected the QA fix action with empty context.diagnostics"
        );

        // With a non-matching diagnostic in context: action is suppressed.
        let unrelated_diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 10,
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
            &[unrelated_diag],
            None,
            RunPhase::Explicit,
        );
        assert!(
            !actions.iter().any(|a| matches!(a,
                CodeActionOrCommand::CodeAction(act) if act.title == "Fix Debian QA group name."
            )),
            "QA fix should be suppressed when context.diagnostics has no matching entry"
        );

        // With the matching diagnostic in context: action is returned and linked.
        let matching_diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 50,
                },
            },
            code: Some(NumberOrString::String(
                "faulty-debian-qa-group-phrase".to_string(),
            )),
            message: "Fix Debian QA group name.".to_string(),
            source: Some("lintian-brush".to_string()),
            ..Default::default()
        };
        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[matching_diag.clone()],
            None,
            RunPhase::Explicit,
        );
        let qa_action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act)
                    if act.title == "Fix Debian QA group name." =>
                {
                    Some(act)
                }
                _ => None,
            })
            .expect("QA fix should be returned when matching diagnostic is in context");
        assert!(
            qa_action
                .diagnostics
                .as_ref()
                .map_or(false, |d| d.contains(&matching_diag)),
            "action should be linked to the matching diagnostic"
        );
    }

    /// Tiny LSP TextEdit applier: convert the LSP range to byte offsets in
    /// `text` and splice in `edit.new_text`. Good enough for these tests.
    fn apply_text_edit_to_string(text: &str, edit: &TextEdit) -> String {
        let start = lsp_pos_to_byte(text, edit.range.start);
        let end = lsp_pos_to_byte(text, edit.range.end);
        let mut out = String::with_capacity(text.len() + edit.new_text.len());
        out.push_str(&text[..start]);
        out.push_str(&edit.new_text);
        out.push_str(&text[end..]);
        out
    }

    #[test]
    fn substitute_emits_one_edit_per_occurrence() {
        let text = "abc PWD def PWD\n";
        let idx = idx_of(text);
        let edits = substitute_edits("PWD", "CURDIR", src_of(text, &idx));
        assert_eq!(edits.len(), 2);
        let mut applied = text.to_string();
        // Apply right-to-left so earlier offsets stay valid.
        for edit in edits.iter().rev() {
            applied = apply_text_edit_to_string(&applied, edit);
        }
        assert_eq!(applied, "abc CURDIR def CURDIR\n");
    }

    #[test]
    fn substitute_with_empty_pattern_emits_nothing() {
        let idx = idx_of("abc");
        assert!(substitute_edits("", "x", src_of("abc", &idx)).is_empty());
    }

    #[test]
    fn remove_paragraph_drops_paragraph_and_separator() {
        let text = "Source: foo\n\nPackage: bar\nDescription: x\n x\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits =
            remove_paragraph_edits(&control, &ParagraphSelector::Source, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(applied, "Package: bar\nDescription: x\n x\n");
    }

    #[test]
    fn append_paragraph_inserts_at_eof_with_separator() {
        let text = "Source: foo\nMaintainer: A B <a@b>\n";
        let idx = idx_of(text);
        let edits = append_paragraph_edits(
            &[
                ("Package".to_string(), "bar".to_string()),
                ("Description".to_string(), "short\nlong line".to_string()),
            ],
            None,
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Source: foo\nMaintainer: A B <a@b>\n\nPackage: bar\nDescription: short\n long line\n",
        );
    }

    #[test]
    fn append_paragraph_skips_separator_when_blank_line_present() {
        let text = "Source: foo\n\n";
        let idx = idx_of(text);
        let edits = append_paragraph_edits(
            &[("Package".to_string(), "bar".to_string())],
            None,
            src_of(text, &idx),
        );
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(applied, "Source: foo\n\nPackage: bar\n");
    }

    #[test]
    fn yaml_set_field_replaces_existing_entry() {
        let text = "Bug-Database: https://example.com/bugs\nRepository: https://example.com/repo\n";
        let yaml_file = yaml_edit::YamlFile::parse(text).to_result().unwrap();
        let action = YamlAction::SetField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: Vec::new(),
            key: "Bug-Database".into(),
            value: "https://newhost/bugs".into(),
        };
        let idx = idx_of(text);
        let edits = yaml_action_to_text_edits(&action, &yaml_file, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Bug-Database: https://newhost/bugs\nRepository: https://example.com/repo\n",
        );
    }

    #[test]
    fn yaml_remove_field_drops_whole_line() {
        let text = "Foo: 1\nBar: 2\nBaz: 3\n";
        let yaml_file = yaml_edit::YamlFile::parse(text).to_result().unwrap();
        let action = YamlAction::RemoveField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: Vec::new(),
            key: "Bar".into(),
        };
        let idx = idx_of(text);
        let edits = yaml_action_to_text_edits(&action, &yaml_file, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(applied, "Foo: 1\nBaz: 3\n");
    }

    #[test]
    fn yaml_rename_field_replaces_only_the_key() {
        let text = "Old-Name: keep-me\n";
        let yaml_file = yaml_edit::YamlFile::parse(text).to_result().unwrap();
        let action = YamlAction::RenameField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: Vec::new(),
            from: "Old-Name".into(),
            to: "New-Name".into(),
        };
        let idx = idx_of(text);
        let edits = yaml_action_to_text_edits(&action, &yaml_file, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(applied, "New-Name: keep-me\n");
    }

    #[test]
    fn changelog_set_entry_date_replaces_just_the_timestamp() {
        let text = "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog = ChangeLog::read_relaxed(text.as_bytes()).unwrap();
        let action = ChangelogAction::SetEntryDate {
            file: PathBuf::from("debian/changelog"),
            version: "1.0".into(),
            rfc2822: "Tue, 02 Jan 2024 12:00:00 +0000".into(),
        };
        let idx = idx_of(text);
        let edits = changelog_action_to_text_edits(&action, &changelog, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(applied.contains("Tue, 02 Jan 2024 12:00:00 +0000"));
        assert!(!applied.contains("Mon, 01 Jan 2024 00:00:00 +0000"));
    }

    #[test]
    fn changelog_replace_entry_changes_swaps_change_block() {
        let text = "foo (1.0) unstable; urgency=medium\n\n  * Old line one.\n  * Old line two.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog = ChangeLog::read_relaxed(text.as_bytes()).unwrap();
        let action = ChangelogAction::ReplaceEntryChanges {
            file: PathBuf::from("debian/changelog"),
            version: "1.0".into(),
            lines: vec!["  * Brand new line.".to_string()],
        };
        let idx = idx_of(text);
        let edits = changelog_action_to_text_edits(&action, &changelog, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(applied.contains("  * Brand new line."));
        assert!(!applied.contains("Old line one"));
        assert!(!applied.contains("Old line two"));
    }

    #[test]
    fn is_action_translatable_filters_setmode_and_systemd() {
        use ::lintian_brush::diagnostic::{DesktopIniAction, SystemdAction};

        let setmode = Action::Filesystem(FilesystemAction::SetMode {
            file: PathBuf::from("debian/rules"),
            mode: 0o755,
        });
        let systemd = Action::Systemd(SystemdAction::SetField {
            file: PathBuf::from("foo.service"),
            section: "Service".into(),
            field: "ExecStart".into(),
            value: "/bin/true".into(),
        });
        let desktop = Action::DesktopIni(DesktopIniAction::SetField {
            file: PathBuf::from("foo.desktop"),
            group: "Desktop Entry".into(),
            field: "Name".into(),
            locale: None,
            value: "Foo".into(),
        });
        let deb822 = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Section".into(),
            value: "misc".into(),
        });
        let delete = Action::Filesystem(FilesystemAction::Delete {
            file: PathBuf::from("debian/pycompat"),
        });

        assert!(!is_action_translatable(&setmode));
        assert!(!is_action_translatable(&systemd));
        assert!(!is_action_translatable(&desktop));
        assert!(is_action_translatable(&deb822));
        assert!(is_action_translatable(&delete));
    }

    #[test]
    fn delete_action_surfaces_as_resource_op() {
        // The `debian-pycompat-is-obsolete` detector emits
        // `FilesystemAction::Delete { file: debian/pycompat }`. With
        // `document_changes` emission we expect that to come through as a
        // `ResourceOp::Delete`, not a TextEdit.
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir(&debian).unwrap();
        std::fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: A <a@b>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        std::fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();
        // The detector only fires when this file exists.
        std::fs::write(debian.join("pycompat"), "2\n").unwrap();

        let mut workspace = Workspace::new();
        let control_uri = Uri::from_file_path(debian.join("control")).unwrap();
        let source_file = workspace.update_file(
            control_uri.clone(),
            std::fs::read_to_string(debian.join("control")).unwrap(),
        );
        let mut open_files = HashMap::new();
        open_files.insert(
            control_uri.clone(),
            FileInfo {
                source_file,
                file_type: FileType::Control,
            },
        );

        let actions = run_fixers_for_uri(
            &control_uri,
            &workspace,
            &open_files,
            &[],
            None,
            RunPhase::Explicit,
        );

        let pycompat_uri = Uri::from_file_path(debian.join("pycompat")).unwrap();
        let action = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(act)
                    if act.title == "Remove obsolete debian/pycompat file." =>
                {
                    Some(act)
                }
                _ => None,
            })
            .expect("expected the pycompat removal action");
        let edit = action
            .edit
            .as_ref()
            .expect("action carries a WorkspaceEdit");
        let DocumentChanges::Operations(ops) = edit
            .document_changes
            .as_ref()
            .expect("document_changes form")
        else {
            panic!("expected DocumentChanges::Operations");
        };
        let has_delete = ops.iter().any(|op| {
            matches!(
                op,
                DocumentChangeOperation::Op(ResourceOp::Delete(d))
                    if d.uri == pycompat_uri
            )
        });
        assert!(
            has_delete,
            "expected a ResourceOp::Delete on the pycompat URI"
        );
    }

    #[test]
    fn changelog_remove_bullet_drops_a_single_bullet() {
        let text = "foo (1.0) unstable; urgency=medium\n\n  * Keep me.\n  * Drop me.\n  * Keep me too.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let changelog = ChangeLog::read_relaxed(text.as_bytes()).unwrap();
        let action = ChangelogAction::RemoveBullet {
            file: PathBuf::from("debian/changelog"),
            version: "1.0".into(),
            author: None,
            text: "* Drop me.".into(),
            occurrence: 0,
        };
        let idx = idx_of(text);
        let edits = changelog_action_to_text_edits(&action, &changelog, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(applied.contains("Keep me."));
        assert!(applied.contains("Keep me too."));
        assert!(!applied.contains("Drop me."));
    }

    #[test]
    fn normalize_field_spacing_collapses_runs_of_whitespace() {
        let text = "Source: foo\nSection:    misc\n\nPackage: bar\nDescription: x\n x\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = normalize_field_spacing_edits(
            &control,
            &ParagraphSelector::Source,
            "Section",
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Source: foo\nSection: misc\n\nPackage: bar\nDescription: x\n x\n",
        );
    }

    #[test]
    fn normalize_field_spacing_skips_already_canonical() {
        let text = "Source: foo\nSection: misc\n\nPackage: bar\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = normalize_field_spacing_edits(
            &control,
            &ParagraphSelector::Source,
            "Section",
            src_of(text, &idx),
        );
        assert!(edits.is_empty());
    }

    #[test]
    fn drop_relation_removes_named_dependency() {
        let text = "Source: foo\nBuild-Depends: debhelper-compat (= 13), unwanted, autoconf\n\nPackage: bar\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = drop_relation_edits(
            &control,
            &ParagraphSelector::Source,
            "Build-Depends",
            "unwanted",
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(!applied.contains("unwanted"));
        assert!(applied.contains("debhelper-compat (= 13)"));
        assert!(applied.contains("autoconf"));
    }

    #[test]
    fn ensure_substvar_appends_when_missing() {
        let text = "Source: foo\n\nPackage: bar\nDepends: libc6\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = ensure_substvar_edits(
            &control,
            &ParagraphSelector::Binary {
                package: "bar".into(),
            },
            "Depends",
            "${misc:Depends}",
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(applied.contains("${misc:Depends}"));
        assert!(applied.contains("libc6"));
    }

    #[test]
    fn ensure_substvar_skips_when_already_present() {
        let text = "Source: foo\n\nPackage: bar\nDepends: libc6, ${misc:Depends}\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = ensure_substvar_edits(
            &control,
            &ParagraphSelector::Binary {
                package: "bar".into(),
            },
            "Depends",
            "${misc:Depends}",
            src_of(text, &idx),
        );
        assert!(edits.is_empty());
    }

    #[test]
    fn ensure_substvar_inserts_field_when_absent() {
        // No Depends field — must insert "Depends: ${misc:Depends}" in
        // canonical BINARY_FIELD_ORDER position (after Architecture, before
        // Description).
        let text =
            "Source: foo\n\nPackage: bar\nArchitecture: any\nDescription: A package\n some text\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = ensure_substvar_edits(
            &control,
            &ParagraphSelector::Binary {
                package: "bar".into(),
            },
            "Depends",
            "${misc:Depends}",
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Source: foo\n\nPackage: bar\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: A package\n some text\n"
        );
    }

    #[test]
    fn drop_substvar_removes_when_present() {
        let text = "Source: foo\n\nPackage: bar\nDepends: libc6, ${shlibs:Depends}\n";
        let parse = Control::parse(text);
        let control = parse.to_result().unwrap();
        let idx = idx_of(text);
        let edits = drop_substvar_edits(
            &control,
            &ParagraphSelector::Binary {
                package: "bar".into(),
            },
            "Depends",
            "${shlibs:Depends}",
            src_of(text, &idx),
        );
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert!(!applied.contains("${shlibs:Depends}"));
        assert!(applied.contains("libc6"));
    }

    #[test]
    fn copyright_set_field_in_header_rewrites_paragraph() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                    Upstream-Name: foo\n\
                    \n\
                    Files: *\n\
                    Copyright: 2024 someone\n\
                    License: GPL-3+\n";
        let copyright: Copyright = text.parse().unwrap();
        let action = Deb822Action::SetField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Upstream-Contact".into(),
            value: "team@example.com".into(),
        };
        let idx = idx_of(text);
        let edits = copyright_action_to_text_edits(&action, copyright, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
             Upstream-Name: foo\n\
             Upstream-Contact: team@example.com\n\
             \n\
             Files: *\n\
             Copyright: 2024 someone\n\
             License: GPL-3+\n"
        );
    }

    #[test]
    fn copyright_set_field_no_op_when_value_unchanged() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                    Upstream-Name: foo\n";
        let copyright: Copyright = text.parse().unwrap();
        let action = Deb822Action::SetField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Upstream-Name".into(),
            value: "foo".into(),
        };
        let idx = idx_of(text);
        let edits = copyright_action_to_text_edits(&action, copyright, src_of(text, &idx));
        assert!(edits.is_empty());
    }

    #[test]
    fn copyright_remove_field_in_files_paragraph() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                    \n\
                    Files: *\n\
                    Copyright: 2024 someone\n\
                    License: GPL-3+\n\
                    Comment: stale\n";
        let copyright: Copyright = text.parse().unwrap();
        let action = Deb822Action::RemoveField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightFiles { glob: "*".into() },
            field: "Comment".into(),
        };
        let idx = idx_of(text);
        let edits = copyright_action_to_text_edits(&action, copyright, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
             \n\
             Files: *\n\
             Copyright: 2024 someone\n\
             License: GPL-3+\n"
        );
    }

    #[test]
    fn copyright_set_license_field_uses_one_space_indent() {
        // DEP-5 mandates a single-space continuation indent for License
        // text. The typed `set_field` enforces it; we verify here by
        // setting a multi-line License value.
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                    \n\
                    License: GPL-3+\n\
                    \n\
                    License: BSD-3-clause\n";
        let copyright: Copyright = text.parse().unwrap();
        let action = Deb822Action::SetField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightLicense {
                name: "BSD-3-clause".into(),
            },
            field: "License".into(),
            value: "BSD-3-clause\nRedistribution and use in source\nand binary forms are OK."
                .into(),
        };
        let idx = idx_of(text);
        let edits = copyright_action_to_text_edits(&action, copyright, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        // Continuation lines must be indented by exactly one space.
        assert_eq!(
            applied,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
             \n\
             License: GPL-3+\n\
             \n\
             License: BSD-3-clause\n \
             Redistribution and use in source\n \
             and binary forms are OK.\n"
        );
    }

    #[test]
    fn copyright_remove_paragraph_drops_files_block() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
                    \n\
                    Files: doomed/*\n\
                    Copyright: 2024 someone\n\
                    License: GPL-3+\n\
                    \n\
                    Files: *\n\
                    Copyright: 2024 someone else\n\
                    License: MIT\n";
        let copyright: Copyright = text.parse().unwrap();
        let action = Deb822Action::RemoveParagraph {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightFiles {
                glob: "doomed/*".into(),
            },
        };
        let idx = idx_of(text);
        let edits = copyright_action_to_text_edits(&action, copyright, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\
             \n\
             Files: *\n\
             Copyright: 2024 someone else\n\
             License: MIT\n"
        );
    }

    #[test]
    fn watch_set_entry_url_v4_swaps_url_in_one_line() {
        let text = "version=4\nopts=foo=bar https://example.com/foo .*-([\\d.]+)\\.tar\\.gz\n";
        let watch = debian_watch::parse::Parse::parse(text).to_watch_file();
        let action = WatchAction::SetEntryUrl {
            file: PathBuf::from("debian/watch"),
            url: "https://example.com/foo".into(),
            new_url: "https://example.com/bar".into(),
        };
        let idx = idx_of(text);
        let edits = watch_action_to_text_edits(&action, watch, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "version=4\nopts=foo=bar https://example.com/bar .*-([\\d.]+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn watch_set_entry_url_no_op_when_already_set() {
        let text = "version=4\nhttps://example.com/foo .*-([\\d.]+)\\.tar\\.gz\n";
        let watch = debian_watch::parse::Parse::parse(text).to_watch_file();
        let action = WatchAction::SetEntryUrl {
            file: PathBuf::from("debian/watch"),
            url: "https://example.com/foo".into(),
            new_url: "https://example.com/foo".into(),
        };
        let idx = idx_of(text);
        let edits = watch_action_to_text_edits(&action, watch, src_of(text, &idx));
        assert!(edits.is_empty());
    }

    #[test]
    fn watch_set_entry_matching_pattern_v5_rewrites_paragraph() {
        let text = "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: \
                    .*-([\\d.]+)\\.tar\\.gz\n";
        let watch = debian_watch::parse::Parse::parse(text).to_watch_file();
        let action = WatchAction::SetEntryMatchingPattern {
            file: PathBuf::from("debian/watch"),
            url: "https://example.com/foo".into(),
            new_pattern: "v(.+)\\.tar\\.gz".into(),
        };
        let idx = idx_of(text);
        let edits = watch_action_to_text_edits(&action, watch, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: v(.+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn watch_remove_entry_option_drops_named_option() {
        let text = "version=4\nopts=mode=git,pretty=raw https://example.com/foo \
                    .*-([\\d.]+)\\.tar\\.gz\n";
        let watch = debian_watch::parse::Parse::parse(text).to_watch_file();
        let action = WatchAction::RemoveEntryOption {
            file: PathBuf::from("debian/watch"),
            url: "https://example.com/foo".into(),
            option: "pretty".into(),
        };
        let idx = idx_of(text);
        let edits = watch_action_to_text_edits(&action, watch, src_of(text, &idx));
        assert_eq!(edits.len(), 1);
        let applied = apply_text_edit_to_string(text, &edits[0]);
        assert_eq!(
            applied,
            "version=4\nopts=mode=git https://example.com/foo .*-([\\d.]+)\\.tar\\.gz\n"
        );
    }

    #[test]
    fn watch_set_entry_option_no_op_when_already_set() {
        let text = "version=4\nopts=mode=git https://example.com/foo .*-([\\d.]+)\\.tar\\.gz\n";
        let watch = debian_watch::parse::Parse::parse(text).to_watch_file();
        let action = WatchAction::SetEntryOption {
            file: PathBuf::from("debian/watch"),
            url: "https://example.com/foo".into(),
            option: "mode".into(),
            value: "git".into(),
        };
        let idx = idx_of(text);
        let edits = watch_action_to_text_edits(&action, watch, src_of(text, &idx));
        assert!(edits.is_empty());
    }

    fn lsp_pos_to_byte(text: &str, pos: Position) -> usize {
        let mut line = 0u32;
        let mut byte = 0usize;
        for (i, c) in text.char_indices() {
            if line == pos.line {
                // Column is in UTF-16 units; for ASCII tests this matches bytes.
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
}
