use std::path::Path;

use ::lintian_brush::diagnostic::{
    Dep3Action, FilesystemAction, LintianOverridesAction, MaintscriptAction, MakefileAction,
    OverrideLineSelector, WatchAction, YamlAction, YamlPathComponent,
};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Range, TextEdit};

use crate::lintian_brush::workspace::LspDebianWorkspace;

use super::deb822_edits::find_entry_in_paragraph;

// ── dep3 ─────────────────────────────────────────────────────────────────────

pub(super) fn dep3_action_to_text_edits(
    action: &Dep3Action,
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    // Use the salsa-cached header parse when the file is open in the
    // editor; fall back to a one-shot parse otherwise (e.g. for files
    // we only know from disk).
    let header_para = if let Some((parse, _)) = ws.parsed_dep3_header_for(rel) {
        let tree = parse.tree();
        let Some(p) = tree.paragraphs().next() else {
            return Vec::new();
        };
        p
    } else {
        let Some((h, _)) = crate::dep3::parse_dep3_header(original_text) else {
            // Empty header — no anchor for
            // SetField/RemoveField/RenameField. For SetField we *could*
            // insert at offset 0; mirror the applier by treating empty
            // header as "no edit" until the user wants it.
            return Vec::new();
        };
        h.as_deb822().clone()
    };
    let paragraph = &header_para;
    match action {
        Dep3Action::SetField { field, value, .. } => {
            // Reuse the deb822 set-field logic by calling
            // `find_entry_in_paragraph` directly on the parsed paragraph.
            if let Some(entry) = find_entry_in_paragraph(paragraph, field) {
                if entry.value().as_str() == value {
                    return Vec::new();
                }
                let Some(value_range) = entry.value_range() else {
                    return Vec::new();
                };
                let lsp_range = original_src.text_range_to_lsp_range(value_range);
                vec![TextEdit {
                    range: lsp_range,
                    new_text: value.clone(),
                }]
            } else {
                // Insert at the end of the header paragraph.
                let para_range = paragraph.text_range();
                let insertion: usize = para_range.end().into();
                let pos = original_src.offset_to_position((insertion as u32).into());
                let new_entry_text = format!("{}: {}\n", field, value);
                vec![TextEdit {
                    range: Range {
                        start: pos,
                        end: pos,
                    },
                    new_text: new_entry_text,
                }]
            }
        }
        Dep3Action::RemoveField { field, .. } => {
            let Some(entry) = find_entry_in_paragraph(paragraph, field) else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(entry.text_range());
            vec![TextEdit {
                range: lsp_range,
                new_text: String::new(),
            }]
        }
        Dep3Action::RenameField {
            from_field,
            to_field,
            ..
        } => {
            let Some(entry) = find_entry_in_paragraph(paragraph, from_field) else {
                return Vec::new();
            };
            let Some(key_range) = entry.key_range() else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(key_range);
            vec![TextEdit {
                range: lsp_range,
                new_text: to_field.clone(),
            }]
        }
    }
}

pub(super) fn dep3_action_range(
    action: &Dep3Action,
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    // Same routing as `dep3_action_to_text_edits`: prefer the
    // salsa-cached header parse, fall back to a one-shot parse for
    // files not tracked by the editor.
    let header_para = if let Some((parse, _)) = ws.parsed_dep3_header_for(rel) {
        parse.tree().paragraphs().next()?
    } else {
        let (h, _) = crate::dep3::parse_dep3_header(anchor_src.text)?;
        h.as_deb822().clone()
    };
    let field = match action {
        Dep3Action::SetField { field, .. } | Dep3Action::RemoveField { field, .. } => {
            field.as_str()
        }
        Dep3Action::RenameField { from_field, .. } => from_field.as_str(),
    };
    let entry = find_entry_in_paragraph(&header_para, field)?;
    Some(anchor_src.text_range_to_lsp_range(entry.text_range()))
}

// ── watch ─────────────────────────────────────────────────────────────────────

/// Find the byte range of a watch entry whose URL matches `url`. Walks
/// the parsed watch file (line-based or deb822) and returns the first
/// matching entry's range — line-based entries cover one line, deb822
/// entries cover one paragraph.
fn watch_entry_range_by_url(
    watch: &debian_watch::parse::ParsedWatchFile,
    url: &str,
) -> Option<rowan::TextRange> {
    use debian_watch::parse::ParsedWatchFile;
    match watch {
        ParsedWatchFile::LineBased(wf) => wf
            .entries()
            .find(|e| e.url() == url)
            .map(|e| e.syntax().text_range()),
        ParsedWatchFile::Deb822(wf) => wf
            .entries()
            .find(|e| e.url() == url)
            .map(|e| e.as_deb822().text_range()),
    }
}

/// Apply `mutate` to the entry whose URL is `url`, then return the
/// entry's post-mutation text and the original byte range it occupied.
/// Returns `None` if no entry matches or `mutate` returns `false`.
fn mutate_watch_entry<F>(
    watch: &mut debian_watch::parse::ParsedWatchFile,
    url: &str,
    mutate: F,
) -> Option<(rowan::TextRange, String)>
where
    F: FnOnce(&mut debian_watch::parse::ParsedEntry) -> bool,
{
    use debian_watch::parse::{ParsedEntry, ParsedWatchFile};
    // We need both: (1) the rowan range BEFORE mutation (since the
    // range coordinates we report to the editor are against the
    // unmodified text), and (2) the post-mutation entry text.
    match watch {
        ParsedWatchFile::LineBased(wf) => {
            for entry in wf.entries() {
                if entry.url() != url {
                    continue;
                }
                let range = entry.syntax().text_range();
                let mut wrapped = ParsedEntry::LineBased(entry);
                if !mutate(&mut wrapped) {
                    return None;
                }
                let ParsedEntry::LineBased(updated) = wrapped else {
                    return None;
                };
                return Some((range, updated.syntax().text().to_string()));
            }
            None
        }
        ParsedWatchFile::Deb822(wf) => {
            for entry in wf.entries() {
                if entry.url() != url {
                    continue;
                }
                let range = entry.as_deb822().text_range();
                let mut wrapped = ParsedEntry::Deb822(entry);
                if !mutate(&mut wrapped) {
                    return None;
                }
                let ParsedEntry::Deb822(updated) = wrapped else {
                    return None;
                };
                return Some((range, updated.as_deb822().to_string()));
            }
            None
        }
    }
}

pub(super) fn watch_action_to_text_edits(
    action: &WatchAction,
    mut watch: debian_watch::parse::ParsedWatchFile,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    use debian_watch::parse::ParsedEntry;

    let result = match action {
        WatchAction::SetEntryMatchingPattern {
            url, new_pattern, ..
        } => mutate_watch_entry(&mut watch, url, |entry| {
            if entry.matching_pattern().as_deref() == Some(new_pattern.as_str()) {
                return false;
            }
            entry.set_matching_pattern(new_pattern);
            true
        }),
        WatchAction::SetEntryUrl { url, new_url, .. } => {
            if url == new_url {
                return Vec::new();
            }
            mutate_watch_entry(&mut watch, url, |entry| {
                entry.set_url(new_url);
                true
            })
        }
        WatchAction::RemoveEntryOption { url, option, .. } => {
            mutate_watch_entry(&mut watch, url, |entry| {
                if entry.get_option(option).is_none() {
                    return false;
                }
                match entry {
                    ParsedEntry::LineBased(e) => {
                        e.del_opt_str(option);
                    }
                    ParsedEntry::Deb822(e) => {
                        e.delete_option_str(option);
                    }
                }
                true
            })
        }
        WatchAction::SetEntryOption {
            url, option, value, ..
        } => mutate_watch_entry(&mut watch, url, |entry| {
            if entry.get_option(option).as_deref() == Some(value.as_str()) {
                return false;
            }
            match entry {
                ParsedEntry::LineBased(e) => e.set_opt(option, value),
                ParsedEntry::Deb822(e) => e.set_option_str(option, value),
            }
            true
        }),
        WatchAction::ConvertEntryToTemplate { url, .. } => {
            mutate_watch_entry(&mut watch, url, |entry| match entry {
                ParsedEntry::Deb822(e) => e.try_convert_to_template().is_some(),
                // Templates are a v5 (deb822) feature only.
                ParsedEntry::LineBased(_) => false,
            })
        }
    };

    let Some((range, new_text)) = result else {
        return Vec::new();
    };
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    if end > original_text.len() || start > end {
        return Vec::new();
    }
    if original_text[start..end] == new_text {
        return Vec::new();
    }
    let lsp_range = original_src.text_range_to_lsp_range(range);
    vec![TextEdit {
        range: lsp_range,
        new_text,
    }]
}

pub(super) fn watch_action_range(
    action: &WatchAction,
    watch: &debian_watch::parse::ParsedWatchFile,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    let url = match action {
        WatchAction::SetEntryMatchingPattern { url, .. }
        | WatchAction::RemoveEntryOption { url, .. }
        | WatchAction::SetEntryOption { url, .. }
        | WatchAction::SetEntryUrl { url, .. }
        | WatchAction::ConvertEntryToTemplate { url, .. } => url,
    };
    let range = watch_entry_range_by_url(watch, url)?;
    Some(anchor_src.text_range_to_lsp_range(range))
}

// ── yaml ──────────────────────────────────────────────────────────────────────

pub(super) fn yaml_action_to_text_edits(
    action: &YamlAction,
    yaml_file: &yaml_edit::YamlFile,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    let Some(doc) = yaml_file.document() else {
        return Vec::new();
    };
    let Some(parent) = navigate_yaml_mapping(&doc, yaml_action_parent_path(action)) else {
        return Vec::new();
    };
    match action {
        YamlAction::SetField { key, value, .. } => {
            yaml_set_field_edits(&parent, key, value, None, original_src)
        }
        YamlAction::SetFieldOrdered {
            key,
            value,
            field_order,
            ..
        } => yaml_set_field_edits(&parent, key, value, Some(field_order), original_src),
        YamlAction::RemoveField { key, .. } => {
            let Some(entry) = parent.find_entry_by_key(key.as_str()) else {
                return Vec::new();
            };
            let entry_range = entry.syntax().text_range();
            let start: usize = entry_range.start().into();
            let end_after_nl = absorb_trailing_newline(original_text, entry_range.end().into());
            let text_range =
                rowan::TextRange::new((start as u32).into(), (end_after_nl as u32).into());
            let lsp_range = original_src.text_range_to_lsp_range(text_range);
            vec![TextEdit {
                range: lsp_range,
                new_text: String::new(),
            }]
        }
        YamlAction::RenameField { from, to, .. } => {
            let Some(entry) = parent.find_entry_by_key(from.as_str()) else {
                return Vec::new();
            };
            let Some(key_node) = entry.key_node() else {
                return Vec::new();
            };
            let key_syntax_range = match &key_node {
                yaml_edit::YamlNode::Scalar(s) => s.syntax().text_range(),
                yaml_edit::YamlNode::Mapping(m) => m.syntax().text_range(),
                yaml_edit::YamlNode::Sequence(s) => s.syntax().text_range(),
                yaml_edit::YamlNode::Alias(a) => a.syntax().text_range(),
                yaml_edit::YamlNode::TaggedNode(t) => t.syntax().text_range(),
            };
            let lsp_range = original_src.text_range_to_lsp_range(key_syntax_range);
            vec![TextEdit {
                range: lsp_range,
                new_text: to.clone(),
            }]
        }
    }
}

/// Edits for `YamlAction::SetField` / `SetFieldOrdered`. When the key
/// already exists, replace just its entry. When inserting, place the new
/// entry according to `field_order` (if given) — keys earlier in the list
/// come first; keys not listed land at the end.
fn yaml_set_field_edits(
    parent: &yaml_edit::Mapping,
    key: &str,
    value: &str,
    field_order: Option<&[String]>,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    if let Some(entry) = parent.find_entry_by_key(key) {
        if let Some(yaml_edit::YamlNode::Scalar(scalar)) = entry.value_node() {
            if scalar.as_string() == value {
                return Vec::new();
            }
        }
        let entry_range = entry.syntax().text_range();
        let lsp_range = original_src.text_range_to_lsp_range(entry_range);
        return vec![TextEdit {
            range: lsp_range,
            new_text: format_yaml_entry(key, value, false),
        }];
    }

    // Field is missing. Decide where to insert.
    let insertion_offset = match field_order {
        Some(order) => yaml_ordered_insertion_offset(parent, key, order)
            .unwrap_or_else(|| parent.syntax().text_range().end().into()),
        None => parent.syntax().text_range().end().into(),
    };
    let pos = original_src.offset_to_position((insertion_offset as u32).into());
    let leading_newline =
        insertion_offset > 0 && !original_text[..insertion_offset].ends_with('\n');
    let new_text = format_yaml_entry(key, value, leading_newline);
    vec![TextEdit {
        range: Range {
            start: pos,
            end: pos,
        },
        new_text,
    }]
}

/// Find the byte offset at which to insert `key` to honour `field_order`.
/// Returns the start offset of the first existing entry whose order index
/// is greater than `key`'s, or `None` to fall back to end-of-mapping.
fn yaml_ordered_insertion_offset(
    parent: &yaml_edit::Mapping,
    key: &str,
    field_order: &[String],
) -> Option<usize> {
    let key_idx = field_order.iter().position(|k| k == key)?;
    for entry in parent.entries() {
        let Some(entry_key_node) = entry.key_node() else {
            continue;
        };
        let entry_key = match &entry_key_node {
            yaml_edit::YamlNode::Scalar(s) => s.as_string(),
            _ => continue,
        };
        let Some(other_idx) = field_order.iter().position(|k| *k == entry_key) else {
            continue;
        };
        if other_idx > key_idx {
            let start: usize = entry.syntax().text_range().start().into();
            return Some(start);
        }
    }
    None
}

pub(super) fn yaml_action_range(
    action: &YamlAction,
    yaml_file: &yaml_edit::YamlFile,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    let doc = yaml_file.document()?;
    let parent = navigate_yaml_mapping(&doc, yaml_action_parent_path(action))?;
    let key = match action {
        YamlAction::SetField { key, .. }
        | YamlAction::SetFieldOrdered { key, .. }
        | YamlAction::RemoveField { key, .. } => key.as_str(),
        YamlAction::RenameField { from, .. } => from.as_str(),
    };
    let entry = parent.find_entry_by_key(key)?;
    Some(anchor_src.text_range_to_lsp_range(entry.syntax().text_range()))
}

fn yaml_action_parent_path(action: &YamlAction) -> &[YamlPathComponent] {
    match action {
        YamlAction::SetField { parent_path, .. }
        | YamlAction::SetFieldOrdered { parent_path, .. }
        | YamlAction::RemoveField { parent_path, .. }
        | YamlAction::RenameField { parent_path, .. } => parent_path.as_slice(),
    }
}

/// Walk down a YAML document along `path` and return the mapping at that
/// location. Mirrors `lintian_brush::appliers::navigate_yaml_mapping` but
/// in read-only form, since detector ranges never insert new mappings.
fn navigate_yaml_mapping(
    doc: &yaml_edit::Document,
    path: &[YamlPathComponent],
) -> Option<yaml_edit::Mapping> {
    let mut mapping = doc.as_mapping()?;
    for component in path {
        match component {
            YamlPathComponent::Key { key } => {
                mapping = mapping.get_mapping(key.as_str())?;
            }
            // Sequence-index components aren't supported by the applier
            // either; bail out the same way.
            YamlPathComponent::Index { .. } => return None,
        }
    }
    Some(mapping)
}

/// Render a `key: value` YAML entry with no fancy quoting. Caller adds
/// any leading newline; we always emit a trailing newline.
fn format_yaml_entry(key: &str, value: &str, leading_newline: bool) -> String {
    let lead = if leading_newline { "\n" } else { "" };
    format!("{lead}{key}: {value}\n")
}

/// Extend `end` past one newline, so removing `[start..end]` leaves a
/// clean line break and not an empty line.
fn absorb_trailing_newline(text: &str, end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = end;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\n' {
        i += 1;
    } else if i + 1 < bytes.len() && bytes[i] == b'\r' && bytes[i + 1] == b'\n' {
        i += 2;
    }
    i
}

// ── makefile ──────────────────────────────────────────────────────────────────

/// Translate a [`MakefileAction`] into `TextEdit`s against `original_src`.
///
/// Clones the salsa-cached `Makefile` tree, applies the mutation, then emits a
/// whole-document replacement when the text changed. Makefile files are
/// typically short so the full replacement is safe and keeps the translator
/// simple.
pub(super) fn makefile_action_to_text_edits(
    action: &MakefileAction,
    makefile: &makefile_lossless::Makefile,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        MakefileAction::ReplaceRecipe {
            target,
            recipe,
            new_recipe,
            ..
        } => {
            let mf = makefile.clone();
            for mut rule in mf.rules_by_target(target) {
                let recipes: Vec<String> = rule.recipes().collect();
                if let Some(idx) = recipes.iter().position(|r| r == recipe) {
                    if rule.replace_command(idx, new_recipe) {
                        break;
                    }
                }
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::RemoveRecipe { target, recipe, .. } => {
            let mf = makefile.clone();
            for mut rule in mf.rules_by_target(target) {
                let recipes: Vec<String> = rule.recipes().collect();
                if let Some(idx) = recipes.iter().position(|r| r == recipe) {
                    if rule.remove_command(idx) {
                        break;
                    }
                }
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::SetVariable { name, value, .. } => {
            let mf = makefile.clone();
            if let Some(mut var) = mf.find_variable(name).next() {
                var.set_value(value);
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::SetVariableOperator {
            name, operator, ..
        } => {
            let mf = makefile.clone();
            if let Some(mut var) = mf.find_variable(name).next() {
                var.set_assignment_operator(operator);
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::RemoveVariable { name, .. } => {
            let mf = makefile.clone();
            if let Some(mut var) = mf.find_variable(name).next() {
                var.remove();
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::RemoveRule { target, .. } => {
            let mut mf = makefile.clone();
            let idx = mf
                .rules()
                .enumerate()
                .find(|(_, r)| r.targets().any(|t| t == target.as_str()))
                .map(|(i, _)| i);
            if let Some(i) = idx {
                let _ = mf.remove_rule(i);
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::RemovePhonyTarget { target, .. } => {
            let mut mf = makefile.clone();
            let _ = mf.remove_phony_target(target);
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::RenameRuleTarget {
            from_target,
            to_target,
            ..
        } => {
            let mf = makefile.clone();
            for mut rule in mf.rules().collect::<Vec<_>>() {
                let _ = rule.rename_target(from_target, to_target);
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::AddRule {
            target,
            prerequisites,
            ..
        } => {
            let mut mf = makefile.clone();
            let prereqs: Vec<&str> = prerequisites.iter().map(String::as_str).collect();
            let mut rule = mf.add_rule(target);
            let _ = rule.set_prerequisites(prereqs);
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::AddPhonyTarget { target, .. } => {
            let mut mf = makefile.clone();
            let _ = mf.add_phony_target(target);
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::AddInclude { path, .. } => {
            let mut mf = makefile.clone();
            mf.add_include(path);
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::ReplaceVariableWithInclude { name, path, .. } => {
            let mut mf = makefile.clone();
            let var_idx = mf
                .variable_definitions()
                .enumerate()
                .find(|(_, v)| v.name().as_deref() == Some(name.as_str()))
                .map(|(i, _)| i);
            if let Some(idx) = var_idx {
                let _ = mf.insert_include(idx, path);
                let _ = mf.find_variable(name).next().map(|mut v| v.remove());
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
        MakefileAction::InsertIncludeBeforeVariable {
            path,
            before_variable,
            ..
        } => {
            let mut mf = makefile.clone();
            let var_idx = mf
                .variable_definitions()
                .enumerate()
                .find(|(_, v)| v.name().as_deref() == Some(before_variable.as_str()))
                .map(|(i, _)| i);
            if let Some(idx) = var_idx {
                let _ = mf.insert_include(idx, path);
            }
            makefile_diff_edits(makefile, &mf, original_src)
        }
    }
}

fn makefile_diff_edits(
    before: &makefile_lossless::Makefile,
    after: &makefile_lossless::Makefile,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let before_text = before.to_string();
    let after_text = after.to_string();
    if before_text == after_text {
        return Vec::new();
    }
    vec![TextEdit {
        range: super::translate::full_document_range(original_src.text),
        new_text: after_text,
    }]
}

pub(super) fn makefile_action_range(
    action: &MakefileAction,
    makefile: &makefile_lossless::Makefile,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    match action {
        MakefileAction::ReplaceRecipe { target, recipe, .. }
        | MakefileAction::RemoveRecipe { target, recipe, .. } => {
            let rule = makefile.rules_by_target(target).next()?;
            let node = rule.recipe_nodes().find(|r| r.text() == *recipe)?;
            Some(anchor_src.text_range_to_lsp_range(node.text_range()))
        }
        MakefileAction::SetVariable { name, .. }
        | MakefileAction::SetVariableOperator { name, .. }
        | MakefileAction::RemoveVariable { name, .. }
        | MakefileAction::ReplaceVariableWithInclude { name, .. }
        | MakefileAction::InsertIncludeBeforeVariable {
            before_variable: name,
            ..
        } => {
            use rowan::ast::AstNode as _;
            let var = makefile.find_variable(name).next()?;
            Some(anchor_src.text_range_to_lsp_range(var.syntax().text_range()))
        }
        MakefileAction::RemoveRule { target, .. }
        | MakefileAction::RenameRuleTarget {
            from_target: target,
            ..
        }
        | MakefileAction::AddRule { target, .. }
        | MakefileAction::AddPhonyTarget { target, .. }
        | MakefileAction::RemovePhonyTarget { target, .. } => {
            use rowan::ast::AstNode as _;
            let rule = makefile.rules_by_target(target).next()?;
            Some(anchor_src.text_range_to_lsp_range(rule.syntax().text_range()))
        }
        MakefileAction::AddInclude { .. } => None,
    }
}

// ── lintian-overrides ─────────────────────────────────────────────────────────

pub(super) fn lintian_overrides_action_to_text_edits(
    action: &LintianOverridesAction,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    use lintian_overrides::LintianOverrides;
    let text = original_src.text;
    let Ok(parsed) = LintianOverrides::parse(text).ok() else {
        return Vec::new();
    };
    match action {
        LintianOverridesAction::AddLine {
            package, tag, info, ..
        } => {
            let already_present = parsed.lines().any(|line| {
                if line.is_comment() || line.is_empty() {
                    return false;
                }
                let line_tag = match line.tag() {
                    Some(t) => t.text().to_string(),
                    None => return false,
                };
                if line_tag != *tag {
                    return false;
                }
                let line_pkg = line
                    .package_spec()
                    .as_ref()
                    .and_then(|s| s.package_name());
                if line_pkg.as_deref() != package.as_deref() {
                    return false;
                }
                line.info().as_deref() == info.as_deref()
            });
            if already_present {
                return Vec::new();
            }
            let mut new_line = String::new();
            if !text.is_empty() && !text.ends_with('\n') {
                new_line.push('\n');
            }
            if let Some(pkg) = package {
                new_line.push_str(pkg);
                new_line.push_str(": ");
            }
            new_line.push_str(tag);
            if let Some(i) = info {
                new_line.push(' ');
                new_line.push_str(i);
            }
            new_line.push('\n');
            let end = rowan::TextSize::of(text);
            let insertion = rowan::TextRange::new(end, end);
            vec![TextEdit {
                range: original_src.text_range_to_lsp_range(insertion),
                new_text: new_line,
            }]
        }
        LintianOverridesAction::DropLine { selector, .. } => {
            let Some(line) = find_override_line(&parsed, selector) else {
                return Vec::new();
            };
            let range = line_node_range_with_newline(text, line.text_range());
            vec![TextEdit {
                range: original_src.text_range_to_lsp_range(range),
                new_text: String::new(),
            }]
        }
        LintianOverridesAction::RenameTag {
            from_tag, to_tag, ..
        } => {
            let lines: Vec<_> = parsed
                .lines()
                .filter(|l| l.tag().as_ref().map(|t| t.text()) == Some(from_tag.as_str()))
                .collect();
            let mut edits = Vec::new();
            for line in lines {
                if let Some(tag_range) = line.tag_range() {
                    edits.push(TextEdit {
                        range: original_src.text_range_to_lsp_range(tag_range),
                        new_text: to_tag.clone(),
                    });
                }
            }
            edits
        }
        LintianOverridesAction::SetLineInfo {
            selector, new_info, ..
        } => {
            let Some(line) = find_override_line(&parsed, selector) else {
                return Vec::new();
            };
            if let Some(info_range) = line.info_range() {
                vec![TextEdit {
                    range: original_src.text_range_to_lsp_range(info_range),
                    new_text: new_info.clone(),
                }]
            } else if new_info.is_empty() {
                Vec::new()
            } else {
                // No existing info — append after the tag.
                let Some(tag_range) = line.tag_range() else {
                    return Vec::new();
                };
                let insertion = rowan::TextRange::new(tag_range.end(), tag_range.end());
                vec![TextEdit {
                    range: original_src.text_range_to_lsp_range(insertion),
                    new_text: format!(" {}", new_info),
                }]
            }
        }
    }
}

fn find_override_line(
    parsed: &lintian_overrides::LintianOverrides,
    selector: &OverrideLineSelector,
) -> Option<lintian_overrides::OverrideLine> {
    parsed.lines().find(|l| {
        l.tag().as_ref().map(|t| t.text()) == Some(selector.tag.as_str())
            && l.info().as_deref() == selector.info.as_deref()
            && l.package().as_deref() == selector.package.as_deref()
    })
}

/// Extend a node's byte range to include the trailing newline, if present.
/// This ensures `DropLine` deletes the whole line rather than leaving a blank
/// line behind.
fn line_node_range_with_newline(text: &str, range: rowan::TextRange) -> rowan::TextRange {
    let end: usize = range.end().into();
    if text.as_bytes().get(end) == Some(&b'\n') {
        rowan::TextRange::new(range.start(), (end + 1).try_into().unwrap_or(range.end()))
    } else {
        range
    }
}

pub(super) fn lintian_overrides_action_range(
    action: &LintianOverridesAction,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    use lintian_overrides::LintianOverrides;
    let parsed = LintianOverrides::parse(anchor_src.text).ok().ok()?;
    match action {
        LintianOverridesAction::AddLine { .. } => None,
        LintianOverridesAction::DropLine { selector, .. }
        | LintianOverridesAction::SetLineInfo { selector, .. } => {
            let line = find_override_line(&parsed, selector)?;
            Some(anchor_src.text_range_to_lsp_range(line.text_range()))
        }
        LintianOverridesAction::RenameTag { from_tag, .. } => {
            let line = parsed
                .lines()
                .find(|l| l.tag().as_ref().map(|t| t.text()) == Some(from_tag.as_str()))?;
            let range = line.tag_range()?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
    }
}

// ── maintscript ───────────────────────────────────────────────────────────────

pub(super) fn maintscript_action_to_text_edits(
    action: &MaintscriptAction,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        MaintscriptAction::DropEntry { entry, .. } => {
            let Some(range) = find_maintscript_entry_range(original_src.text, entry) else {
                return Vec::new();
            };
            vec![TextEdit {
                range: original_src.text_range_to_lsp_range(range),
                new_text: String::new(),
            }]
        }
    }
}

pub(super) fn maintscript_action_range(
    action: &MaintscriptAction,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    match action {
        MaintscriptAction::DropEntry { entry, .. } => {
            let range = find_maintscript_entry_range(anchor_src.text, entry)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
    }
}

/// Find the byte range of the first maintscript entry whose trimmed text
/// matches `entry`, including any immediately-preceding comment lines and the
/// trailing newline.
fn find_maintscript_entry_range(text: &str, entry: &str) -> Option<rowan::TextRange> {
    let entry_trimmed = entry.trim();
    let mut line_start = 0usize;
    let mut comment_start: Option<usize> = None;

    for line in text.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let trimmed = line.trim_end_matches('\n').trim();
        if trimmed.starts_with('#') {
            if comment_start.is_none() {
                comment_start = Some(line_start);
            }
        } else if trimmed == entry_trimmed {
            let block_start = comment_start.unwrap_or(line_start);
            return Some(rowan::TextRange::new(
                (block_start as u32).into(),
                (line_end as u32).into(),
            ));
        } else {
            comment_start = None;
        }
        line_start = line_end;
    }
    None
}

// ── filesystem / substitute ───────────────────────────────────────────────────

pub(super) fn filesystem_action_to_text_edits(
    action: &FilesystemAction,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    match action {
        FilesystemAction::Write { content, .. } => {
            let Ok(new_text) = std::str::from_utf8(content) else {
                return Vec::new();
            };
            if new_text == original_text {
                return Vec::new();
            }
            vec![TextEdit {
                range: super::translate::full_document_range(original_text),
                new_text: new_text.to_string(),
            }]
        }
        FilesystemAction::ReplaceText {
            range, replacement, ..
        } => {
            if range.start > range.end || range.end > original_text.len() {
                return Vec::new();
            }
            let text_range =
                rowan::TextRange::new((range.start as u32).into(), (range.end as u32).into());
            let lsp_range = original_src.text_range_to_lsp_range(text_range);
            vec![TextEdit {
                range: lsp_range,
                new_text: replacement.clone(),
            }]
        }
        FilesystemAction::NormalizeLineEndings { .. } => {
            // Convert CRLF→LF on the open buffer locally and emit one
            // full-document TextEdit. The Action variant carries the
            // *intent*, the LSP supplies the buffer-precise edit.
            let converted = normalize_crlf(original_text);
            if converted == original_text {
                return Vec::new();
            }
            vec![TextEdit {
                range: super::translate::full_document_range(original_text),
                new_text: converted,
            }]
        }
        FilesystemAction::Substitute { from, to, .. } => substitute_edits(from, to, original_src),
        // The dispatcher in `translate_action` peels these off into
        // resource ops (or panics on SetMode) before reaching here.
        FilesystemAction::Rename { .. }
        | FilesystemAction::Delete { .. }
        | FilesystemAction::RemoveDirIfEmpty { .. }
        | FilesystemAction::SetMode { .. } => {
            unreachable!("resource-op variant routed to filesystem_action_to_text_edits")
        }
    }
}

/// Replace every literal occurrence of `from` with `to` in `original_src`.
/// Mirrors the applier's behaviour: literal find-and-replace, no regex.
fn substitute_edits(
    from: &str,
    to: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    if from.is_empty() {
        return Vec::new();
    }
    let original_text = original_src.text;
    let mut edits = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = original_text[search_from..].find(from) {
        let abs_start = search_from + rel;
        let abs_end = abs_start + from.len();
        let text_range = rowan::TextRange::new((abs_start as u32).into(), (abs_end as u32).into());
        let lsp_range = original_src.text_range_to_lsp_range(text_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: to.to_string(),
        });
        search_from = abs_end;
    }
    edits
}

/// Replace every `\r\n` pair with `\n`, leaving lone `\r`s alone. Same
/// rules as lintian-brush's `appliers::normalize_crlf`.
fn normalize_crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'\r' && bytes[i + 1] == b'\n' {
            out.push('\n');
            i += 2;
        } else {
            // Push the next char (handles multi-byte safely).
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}
