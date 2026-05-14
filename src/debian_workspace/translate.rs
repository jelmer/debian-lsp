use std::path::Path;

use ::lintian_brush::diagnostic::{
    Action, ActionPlan, ChangelogAction, Deb822Action, Dep3Action, FilesystemAction,
    LintianOverridesAction, MaintscriptAction, MakefileAction, WatchAction, YamlAction,
};
use debian_changelog::ChangeLog;
use tower_lsp_server::ls_types::{
    DeleteFile, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};

use crate::debian_workspace::workspace::LspDebianWorkspace;

use super::changelog_edits::{changelog_action_range, changelog_action_to_text_edits};
use super::deb822_edits::{
    copyright_action_to_text_edits, deb822_action_to_text_edits, find_entry_in_paragraph,
    find_paragraph_in_deb822,
};
use super::format_edits::{
    dep3_action_range, dep3_action_to_text_edits, filesystem_action_to_text_edits,
    lintian_overrides_action_range, lintian_overrides_action_to_text_edits,
    maintscript_action_range, maintscript_action_to_text_edits, makefile_action_range,
    makefile_action_to_text_edits, watch_action_range, watch_action_to_text_edits,
    yaml_action_range, yaml_action_to_text_edits,
};

/// Pull the salsa-cached deb822 parse for `rel` if `rel` looks like a
/// deb822 file we can extract a `Deb822` from. Used by the trigger
/// filter to narrow `Deb822Field` triggers to fields whose ranges
/// overlap the changed range. Returns `None` for non-deb822 files.
pub fn parse_for_trigger_filtering_deb822(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<deb822_lossless::Deb822> {
    if rel == Path::new("debian/copyright") {
        ws.parsed_copyright_for(rel).map(|c| c.as_deb822().clone())
    } else {
        ws.parsed_control_for(rel).map(|c| c.as_deb822().clone())
    }
}

pub fn parse_for_trigger_filtering_changelog(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<ChangeLog> {
    if rel == Path::new("debian/changelog") {
        ws.parsed_changelog_for(rel)
    } else {
        None
    }
}

pub fn parse_for_trigger_filtering_yaml(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<yaml_edit::YamlFile> {
    if rel == Path::new("debian/upstream/metadata") {
        ws.parsed_yaml_for(rel)
    } else {
        None
    }
}

pub fn parse_for_trigger_filtering_watch(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<debian_watch::parse::ParsedWatchFile> {
    if rel == Path::new("debian/watch") {
        ws.parsed_watch_for(rel)
    } else {
        None
    }
}

/// Return true if any plan on `diag` has an action targeting `rel`.
pub fn diag_touches_file(diag: &::lintian_brush::diagnostic::Diagnostic, rel: &Path) -> bool {
    diag.plans.iter().any(|plan| {
        plan.actions
            .iter()
            .any(|action| action_file(action) == Some(rel))
    })
}

/// Pick the LSP `Range` to attach to a detector-produced diagnostic.
///
/// We anchor the squiggle on `anchor_rel` (the file the user is
/// currently editing). Walk every action across every plan looking for
/// one that targets `anchor_rel` and produces a precise source range;
/// fall back to a whole-document range if nothing more specific is
/// available.
/// Like `diagnostic_range`, but returns `None` when no action targets
/// the anchor file rather than falling back to the full document range.
/// Used for code-action filtering to avoid showing cross-file actions
/// on every line of the wrong file.
///
/// Filesystem actions (delete/rename/chmod a file) are inherently
/// cross-file: they don't edit any open buffer. Surface them as a
/// zero-length range at line 0 of the anchor file so they're offered
/// regardless of which debian/* file the user has open.
pub fn diagnostic_range_in_file(
    diag: &::lintian_brush::diagnostic::Diagnostic,
    ws: &LspDebianWorkspace<'_>,
    anchor_rel: &Path,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    let mut has_filesystem = false;
    for plan in &diag.plans {
        for action in &plan.actions {
            if matches!(action, Action::Filesystem(_)) {
                has_filesystem = true;
            }
            if action_file(action) != Some(anchor_rel) {
                continue;
            }
            if let Some(range) = locate_action_target(action, ws, anchor_src) {
                return Some(range);
            }
        }
    }
    if has_filesystem {
        let zero = Position::new(0, 0);
        return Some(Range::new(zero, zero));
    }
    None
}

pub fn diagnostic_range(
    diag: &::lintian_brush::diagnostic::Diagnostic,
    ws: &LspDebianWorkspace<'_>,
    anchor_rel: &Path,
    anchor_src: crate::position::Source<'_>,
) -> Range {
    for plan in &diag.plans {
        for action in &plan.actions {
            if action_file(action) != Some(anchor_rel) {
                continue;
            }
            if let Some(range) = locate_action_target(action, ws, anchor_src) {
                return range;
            }
        }
    }
    full_document_range(anchor_src.text)
}

/// Return the LSP `Range` corresponding to the most specific source
/// region the action targets. Walks the salsa-cached AST — never reparses.
pub(super) fn locate_action_target(
    action: &Action,
    ws: &LspDebianWorkspace<'_>,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    let rel = action_file(action)?;
    match action {
        Action::Deb822(deb) => {
            // Find the target paragraph through whichever cached parse
            // matches the file (control vs copyright). Each typed
            // wrapper carries an `as_deb822()` we use for the read-only
            // range probe — the typed setters are only needed for
            // mutations, not for locating ranges.
            let copyright_holder;
            let control_holder;
            let paragraph_in: &deb822_lossless::Deb822 = if rel == Path::new("debian/copyright") {
                copyright_holder = ws.parsed_copyright_for(rel)?;
                copyright_holder.as_deb822()
            } else {
                control_holder = ws.parsed_control_for(rel)?;
                control_holder.as_deb822()
            };
            let (selector, field) = match deb {
                Deb822Action::SetField {
                    paragraph, field, ..
                }
                | Deb822Action::SetFieldWithIndent {
                    paragraph, field, ..
                }
                | Deb822Action::RemoveField {
                    paragraph, field, ..
                }
                | Deb822Action::NormalizeFieldSpacing {
                    paragraph, field, ..
                }
                | Deb822Action::DropRelation {
                    paragraph, field, ..
                }
                | Deb822Action::DropRelationVersionConstraint {
                    paragraph, field, ..
                }
                | Deb822Action::EnsureSubstvar {
                    paragraph, field, ..
                }
                | Deb822Action::DropSubstvar {
                    paragraph, field, ..
                }
                | Deb822Action::EnsureRelation {
                    paragraph, field, ..
                } => (paragraph, Some(field.clone())),
                Deb822Action::ReplaceRelation {
                    paragraph, field, ..
                } => (paragraph, Some(field.clone())),
                Deb822Action::MoveRelation {
                    paragraph,
                    from_field,
                    ..
                } => (paragraph, Some(from_field.clone())),
                Deb822Action::RenameField {
                    paragraph, from, ..
                } => (paragraph, Some(from.clone())),
                Deb822Action::RemoveParagraph { paragraph, .. } => (paragraph, None),
                // These don't anchor on a single existing source range; let
                // the caller fall back to a whole-document range.
                Deb822Action::AppendParagraph { .. } | Deb822Action::ReorderParagraphs { .. } => {
                    return None
                }
            };
            let paragraph = find_paragraph_in_deb822(paragraph_in, selector)?;
            if let Some(field) = field {
                if let Some(entry) = find_entry_in_paragraph(&paragraph, &field) {
                    return Some(anchor_src.text_range_to_lsp_range(entry.text_range()));
                }
            }
            Some(anchor_src.text_range_to_lsp_range(paragraph.text_range()))
        }
        Action::Filesystem(FilesystemAction::ReplaceText { range, .. }) => {
            if range.start > range.end || range.end > anchor_src.text.len() {
                return None;
            }
            let text_range =
                rowan::TextRange::new((range.start as u32).into(), (range.end as u32).into());
            Some(anchor_src.text_range_to_lsp_range(text_range))
        }
        // These filesystem variants don't carry a source range we can map
        // back to a TextEdit position.
        Action::Filesystem(
            FilesystemAction::SetMode { .. }
            | FilesystemAction::Delete { .. }
            | FilesystemAction::Rename { .. }
            | FilesystemAction::RemoveDirIfEmpty { .. }
            | FilesystemAction::Write { .. }
            | FilesystemAction::Substitute { .. }
            | FilesystemAction::NormalizeLineEndings { .. },
        ) => None,
        Action::Yaml(yaml) => {
            let yaml_file = ws.parsed_yaml_for(rel)?;
            yaml_action_range(yaml, &yaml_file, anchor_src)
        }
        Action::Changelog(cl) => {
            let changelog = ws.parsed_changelog_for(rel)?;
            changelog_action_range(cl, &changelog, anchor_src)
        }
        Action::Dep3(d) => dep3_action_range(d, ws, rel, anchor_src),
        Action::Watch(w) => {
            let watch = ws.parsed_watch_for(rel)?;
            watch_action_range(w, &watch, anchor_src)
        }
        Action::Makefile(m) => {
            let makefile = ws.parsed_rules_for(rel)?;
            makefile_action_range(m, &makefile, anchor_src)
        }
        Action::LintianOverrides(ov) => lintian_overrides_action_range(ov, anchor_src),
        Action::Maintscript(ms) => maintscript_action_range(ms, anchor_src),
        // Unwired action kinds — keep the arm exhaustive so upstream additions
        // produce a compile error rather than silent fall-through.
        Action::Systemd(_)
        | Action::DesktopIni(_)
        | Action::Debcargo(_)
        | Action::RunCommand(_) => None,
    }
}

/// Translate an [`ActionPlan`]'s actions into a [`WorkspaceEdit`].
///
/// We always emit the `document_changes` form so a single `WorkspaceEdit`
/// can mix text edits with file-rename / file-delete operations — those
/// are the variants `FilesystemAction::Rename` and `FilesystemAction::Delete`
/// (and `RemoveDirIfEmpty`, which we treat as a delete). Operations are
/// appended in the order the plan lists them so the editor's resolver
/// processes them deterministically.
///
/// Each `Action` dispatches on its kind and walks the salsa-cached parse
/// for its target file to find a byte-precise source range, then emits
/// `TextEdit`s over those ranges. We never reparse here.
pub fn plan_to_workspace_edit(
    plan: &ActionPlan,
    ws: &LspDebianWorkspace<'_>,
) -> Option<WorkspaceEdit> {
    let mut document_changes: Vec<DocumentChangeOperation> = Vec::new();
    // Group consecutive text edits targeting the same file into one
    // `TextDocumentEdit`. We close the group whenever a resource op or a
    // different URI appears, to keep ordering between text and resource
    // ops well-defined.
    let mut pending: Option<(Uri, Vec<TextEdit>)> = None;

    for action in &plan.actions {
        match translate_action(action, ws) {
            ActionEffect::TextEdits { uri, edits } => {
                if edits.is_empty() {
                    continue;
                }
                match &mut pending {
                    Some((p_uri, p_edits)) if *p_uri == uri => {
                        p_edits.extend(edits);
                    }
                    _ => {
                        flush_pending(&mut pending, &mut document_changes);
                        pending = Some((uri, edits));
                    }
                }
            }
            ActionEffect::ResourceOp(op) => {
                flush_pending(&mut pending, &mut document_changes);
                document_changes.push(DocumentChangeOperation::Op(op));
            }
            ActionEffect::None => {}
        }
    }
    flush_pending(&mut pending, &mut document_changes);

    if document_changes.is_empty() {
        None
    } else {
        Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(document_changes)),
            ..Default::default()
        })
    }
}

fn flush_pending(
    pending: &mut Option<(Uri, Vec<TextEdit>)>,
    out: &mut Vec<DocumentChangeOperation>,
) {
    if let Some((uri, edits)) = pending.take() {
        out.push(DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
            edits: edits.into_iter().map(OneOf::Left).collect(),
        }));
    }
}

/// Result of translating a single `Action` into LSP-shaped output. Either
/// a list of text edits scoped to one URI, a single file-level resource
/// operation, or nothing (the action is a no-op against current state).
enum ActionEffect {
    TextEdits { uri: Uri, edits: Vec<TextEdit> },
    ResourceOp(ResourceOp),
    None,
}

/// Return true if `action` is something this translator knows how to
/// turn into either a `TextEdit` or a `ResourceOp`. Plans containing any
/// untranslatable action are dropped so the user never sees a code
/// action whose `translate_action` call would `unimplemented!()`.
pub fn is_action_translatable(action: &Action) -> bool {
    match action {
        // TODO: implement DropRelationVersionConstraint in the translator
        // and flip this to true.
        Action::Deb822(Deb822Action::DropRelationVersionConstraint { .. }) => false,
        Action::Deb822(_) => true,
        Action::Yaml(_) | Action::Changelog(_) => true,
        Action::Filesystem(fs) => match fs {
            FilesystemAction::Write { .. }
            | FilesystemAction::ReplaceText { .. }
            | FilesystemAction::Substitute { .. }
            | FilesystemAction::NormalizeLineEndings { .. }
            | FilesystemAction::Rename { .. }
            | FilesystemAction::Delete { .. }
            | FilesystemAction::RemoveDirIfEmpty { .. } => true,
            // No LSP primitive for chmod.
            FilesystemAction::SetMode { .. } => false,
        },
        // Salsa doesn't track these file types.
        Action::Systemd(_) | Action::DesktopIni(_) => false,
        Action::Dep3(_) | Action::Watch(_) => true,
        Action::Makefile(_) => true,
        Action::LintianOverrides(_) | Action::Maintscript(_) => true,
        // TOML; no cached parse.
        Action::Debcargo(_) => false,
        // No LSP primitive for "run an external command".
        Action::RunCommand(_) => false,
    }
}

fn translate_action(action: &Action, ws: &LspDebianWorkspace<'_>) -> ActionEffect {
    let Some(rel) = action_file(action) else {
        return ActionEffect::None;
    };
    let Some(uri) = ws.resolve_uri(rel) else {
        return ActionEffect::None;
    };

    // Filesystem actions split into two camps: text-edit producing
    // (Write/ReplaceText/Substitute/NormalizeLineEndings) and resource-op
    // producing (Rename/Delete/RemoveDirIfEmpty). SetMode has no LSP
    // equivalent at all — panic loudly there.
    if let Action::Filesystem(fs) = action {
        match fs {
            FilesystemAction::Rename { file, to } => {
                let Some(old_uri) = ws.resolve_uri(file) else {
                    return ActionEffect::None;
                };
                let Some(new_uri) = ws.resolve_uri(to) else {
                    return ActionEffect::None;
                };
                return ActionEffect::ResourceOp(ResourceOp::Rename(RenameFile {
                    old_uri,
                    new_uri,
                    options: None,
                    annotation_id: None,
                }));
            }
            FilesystemAction::Delete { file } | FilesystemAction::RemoveDirIfEmpty { file } => {
                let Some(uri) = ws.resolve_uri(file) else {
                    return ActionEffect::None;
                };
                return ActionEffect::ResourceOp(ResourceOp::Delete(DeleteFile {
                    uri,
                    options: None,
                    annotation_id: None,
                }));
            }
            FilesystemAction::SetMode { .. } => {
                unimplemented!("FilesystemAction::SetMode has no LSP equivalent")
            }
            FilesystemAction::Write { .. }
            | FilesystemAction::ReplaceText { .. }
            | FilesystemAction::Substitute { .. }
            | FilesystemAction::NormalizeLineEndings { .. } => {
                let original = ws.current_text(rel).unwrap_or_default();
                let original_idx = crate::position::LineIndex::new(&original);
                let original_src = crate::position::Source::new(&original, &original_idx);
                return ActionEffect::TextEdits {
                    uri,
                    edits: filesystem_action_to_text_edits(fs, original_src),
                };
            }
        }
    }

    // All other actions produce text edits.
    let original = ws.current_text(rel).unwrap_or_default();
    let original_idx = crate::position::LineIndex::new(&original);
    let original_src = crate::position::Source::new(&original, &original_idx);
    let edits = action_to_text_edits(action, ws, rel, original_src);
    ActionEffect::TextEdits { uri, edits }
}

/// Translate one text-edit-producing `Action` into byte-precise
/// `TextEdit`s. Called from `translate_action` after it has peeled off
/// the resource-op-shaped `Filesystem` variants.
fn action_to_text_edits(
    action: &Action,
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        Action::Deb822(deb) => {
            // Route to the right cached parse based on the target file.
            // debian/copyright has its own typed wrappers (Header /
            // FilesParagraph / LicenseParagraph) that honour DEP-5 field
            // ordering and the License-field 1-space indent rule;
            // bypassing them and editing through plain deb822 would lose
            // those guarantees. Everything else (debian/control,
            // debian/tests/control, ...) goes through the control path.
            if rel == Path::new("debian/copyright") {
                let Some(copyright) = ws.parsed_copyright_for(rel) else {
                    return Vec::new();
                };
                copyright_action_to_text_edits(deb, copyright, original_src)
            } else {
                let Some(control) = ws.parsed_control_for(rel) else {
                    return Vec::new();
                };
                deb822_action_to_text_edits(deb, &control, original_src)
            }
        }
        Action::Filesystem(fs) => filesystem_action_to_text_edits(fs, original_src),
        Action::Yaml(yaml) => {
            let Some(yaml_file) = ws.parsed_yaml_for(rel) else {
                return Vec::new();
            };
            yaml_action_to_text_edits(yaml, &yaml_file, original_src)
        }
        Action::Changelog(cl) => {
            let Some(changelog) = ws.parsed_changelog_for(rel) else {
                return Vec::new();
            };
            changelog_action_to_text_edits(cl, &changelog, original_src)
        }
        // Systemd and DesktopIni files aren't tracked by the salsa
        // workspace yet — adding new file types is a bigger change. Until
        // that lands, panic loudly so a detector emitting one of these
        // doesn't disappear into thin air.
        Action::Systemd(_) => unimplemented!(
            "Systemd actions are not yet wired into the LSP translator (no salsa parse)"
        ),
        Action::DesktopIni(_) => unimplemented!(
            "DesktopIni actions are not yet wired into the LSP translator (no salsa parse)"
        ),
        Action::Dep3(d) => dep3_action_to_text_edits(d, ws, rel, original_src),
        Action::Watch(w) => {
            let Some(watch) = ws.parsed_watch_for(rel) else {
                return Vec::new();
            };
            watch_action_to_text_edits(w, watch, original_src)
        }
        Action::Makefile(m) => {
            let Some(makefile) = ws.parsed_rules_for(rel) else {
                return Vec::new();
            };
            makefile_action_to_text_edits(m, &makefile, original_src)
        }
        Action::LintianOverrides(ov) => lintian_overrides_action_to_text_edits(ov, original_src),
        Action::Maintscript(ms) => maintscript_action_to_text_edits(ms, original_src),
        Action::Debcargo(_) => {
            unimplemented!("Debcargo actions not yet wired into the LSP translator")
        }
        Action::RunCommand(_) => {
            unimplemented!("RunCommand actions have no LSP equivalent")
        }
    }
}

pub(super) fn action_file(action: &Action) -> Option<&Path> {
    Some(match action {
        Action::Deb822(a) => match a {
            Deb822Action::SetField { file, .. }
            | Deb822Action::SetFieldWithIndent { file, .. }
            | Deb822Action::RemoveField { file, .. }
            | Deb822Action::RenameField { file, .. }
            | Deb822Action::RemoveParagraph { file, .. }
            | Deb822Action::AppendParagraph { file, .. }
            | Deb822Action::NormalizeFieldSpacing { file, .. }
            | Deb822Action::DropRelation { file, .. }
            | Deb822Action::DropRelationVersionConstraint { file, .. }
            | Deb822Action::ReplaceRelation { file, .. }
            | Deb822Action::EnsureSubstvar { file, .. }
            | Deb822Action::DropSubstvar { file, .. }
            | Deb822Action::EnsureRelation { file, .. }
            | Deb822Action::MoveRelation { file, .. }
            | Deb822Action::ReorderParagraphs { file, .. } => file,
        },
        Action::Filesystem(a) => match a {
            FilesystemAction::SetMode { file, .. }
            | FilesystemAction::Delete { file }
            | FilesystemAction::Rename { file, .. }
            | FilesystemAction::RemoveDirIfEmpty { file }
            | FilesystemAction::Write { file, .. }
            | FilesystemAction::ReplaceText { file, .. }
            | FilesystemAction::Substitute { file, .. }
            | FilesystemAction::NormalizeLineEndings { file } => file,
        },
        Action::Yaml(a) => match a {
            YamlAction::SetField { file, .. }
            | YamlAction::SetFieldOrdered { file, .. }
            | YamlAction::RemoveField { file, .. }
            | YamlAction::RenameField { file, .. } => file,
        },
        Action::Changelog(a) => match a {
            ChangelogAction::ReplaceEntryChanges { file, .. }
            | ChangelogAction::SetEntryDate { file, .. }
            | ChangelogAction::RemoveBullet { file, .. }
            | ChangelogAction::ReplaceBullet { file, .. }
            | ChangelogAction::SetEntryVersion { file, .. } => file,
        },
        Action::Dep3(a) => match a {
            Dep3Action::SetField { file, .. }
            | Dep3Action::RemoveField { file, .. }
            | Dep3Action::RenameField { file, .. } => file,
        },
        Action::Watch(a) => match a {
            WatchAction::SetEntryMatchingPattern { file, .. }
            | WatchAction::RemoveEntryOption { file, .. }
            | WatchAction::SetEntryOption { file, .. }
            | WatchAction::SetEntryUrl { file, .. }
            | WatchAction::ConvertEntryToTemplate { file, .. } => file,
        },
        Action::Makefile(a) => match a {
            MakefileAction::ReplaceRecipe { file, .. }
            | MakefileAction::RemoveRecipe { file, .. }
            | MakefileAction::SetVariable { file, .. }
            | MakefileAction::SetVariableOperator { file, .. }
            | MakefileAction::RemoveVariable { file, .. }
            | MakefileAction::RemoveRule { file, .. }
            | MakefileAction::RemovePhonyTarget { file, .. }
            | MakefileAction::RenameRuleTarget { file, .. }
            | MakefileAction::AddRule { file, .. }
            | MakefileAction::AddPhonyTarget { file, .. }
            | MakefileAction::AddInclude { file, .. }
            | MakefileAction::ReplaceVariableWithInclude { file, .. }
            | MakefileAction::InsertIncludeBeforeVariable { file, .. } => file,
        },
        Action::LintianOverrides(a) => match a {
            LintianOverridesAction::AddLine { file, .. }
            | LintianOverridesAction::DropLine { file, .. }
            | LintianOverridesAction::RenameTag { file, .. }
            | LintianOverridesAction::SetLineInfo { file, .. } => file,
        },
        Action::Maintscript(a) => match a {
            MaintscriptAction::DropEntry { file, .. } => file,
        },
        // These action kinds aren't wired into the LSP translator
        // (see `is_action_translatable` for rationale).
        Action::Systemd(_)
        | Action::DesktopIni(_)
        | Action::Debcargo(_)
        | Action::RunCommand(_) => return None,
    })
}

/// Compute a `Range` covering the entire document, in LSP coordinates
/// (line / utf-16 character).
pub(super) fn full_document_range(text: &str) -> Range {
    let mut last_line = 0u32;
    let mut last_line_start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            last_line += 1;
            last_line_start = i + 1;
        }
    }
    let last_chars = text[last_line_start..].encode_utf16().count() as u32;
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: last_line,
            character: last_chars,
        },
    }
}
