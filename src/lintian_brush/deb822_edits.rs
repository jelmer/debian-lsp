use std::collections::HashMap;

use ::lintian_brush::diagnostic::{
    Deb822Action, IndentPattern, ParagraphSelector,
};
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use tower_lsp_server::ls_types::{Position, Range, TextEdit};

pub(super) fn deb822_action_to_text_edits(
    action: &Deb822Action,
    control: &Control,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        Deb822Action::SetField {
            paragraph,
            field,
            value,
            ..
        } => set_field_edits(control, paragraph, field, value, original_src),
        Deb822Action::RemoveField {
            paragraph, field, ..
        } => remove_field_edits(control, paragraph, field, original_src),
        Deb822Action::RenameField {
            paragraph,
            from,
            to,
            ..
        } => rename_field_edits(control, paragraph, from, to, original_src),
        Deb822Action::RemoveParagraph { paragraph, .. } => {
            remove_paragraph_edits(control, paragraph, original_src)
        }
        Deb822Action::AppendParagraph { fields, indent, .. } => {
            append_paragraph_edits(fields, *indent, original_src)
        }
        Deb822Action::NormalizeFieldSpacing {
            paragraph, field, ..
        } => normalize_field_spacing_edits(control, paragraph, field, original_src),
        Deb822Action::DropRelation {
            paragraph,
            field,
            package,
            ..
        } => drop_relation_edits(control, paragraph, field, package, original_src),
        Deb822Action::EnsureSubstvar {
            paragraph,
            field,
            substvar,
            ..
        } => ensure_substvar_edits(control, paragraph, field, substvar, original_src),
        Deb822Action::DropSubstvar {
            paragraph,
            field,
            substvar,
            ..
        } => drop_substvar_edits(control, paragraph, field, substvar, original_src),
        Deb822Action::SetFieldWithIndent {
            paragraph,
            field,
            value,
            indent,
            ..
        } => set_field_with_indent_edits(control, paragraph, field, value, indent, original_src),
        Deb822Action::ReplaceRelation {
            paragraph,
            field,
            from_package,
            to_entry,
            ..
        } => replace_relation_edits(
            control,
            paragraph,
            field,
            from_package,
            to_entry,
            original_src,
        ),
        Deb822Action::EnsureRelation {
            paragraph,
            field,
            entry,
            ..
        } => ensure_relation_edits(control, paragraph, field, entry, original_src),
        Deb822Action::MoveRelation {
            paragraph,
            from_field,
            to_field,
            package,
            ..
        } => move_relation_edits(
            control,
            paragraph,
            from_field,
            to_field,
            package,
            original_src,
        ),
        Deb822Action::ReorderParagraphs {
            key_field, order, ..
        } => reorder_paragraphs_edits(control, key_field, order, original_src),
    }
}

/// Translate a `Deb822Action` against `debian/copyright` into a single
/// `TextEdit` that rewrites the affected paragraph in place.
///
/// We don't do byte-precise per-field edits here because the typed
/// copyright wrappers (`Header`, `FilesParagraph`, `LicenseParagraph`)
/// have to honour DEP-5's field ordering and the License-field 1-space
/// indent rule — going through them is the only way to keep those
/// guarantees. We mutate the cached green tree (which `tree()` returns
/// in mutable form), snapshot the target paragraph's range *before*
/// mutation, then emit one TextEdit with the post-mutation paragraph
/// text. No reparsing.
pub(super) fn copyright_action_to_text_edits(
    action: &Deb822Action,
    copyright: Copyright,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    // AppendParagraph and ReorderParagraphs don't target a single
    // existing paragraph; route them through the generic helpers (which
    // operate on raw text or on a Deb822 directly).
    match action {
        Deb822Action::AppendParagraph { fields, indent, .. } => {
            return append_paragraph_edits(fields, *indent, original_src);
        }
        Deb822Action::ReorderParagraphs { .. } => {
            // reorder_paragraphs_edits operates on Control, not Copyright.
            // Returning empty here causes plan_to_workspace_edit to return
            // None, so the code action is not offered.
            return Vec::new();
        }
        _ => {}
    }

    let selector = match action {
        Deb822Action::SetField { paragraph, .. }
        | Deb822Action::SetFieldWithIndent { paragraph, .. }
        | Deb822Action::RemoveField { paragraph, .. }
        | Deb822Action::RenameField { paragraph, .. }
        | Deb822Action::RemoveParagraph { paragraph, .. }
        | Deb822Action::NormalizeFieldSpacing { paragraph, .. } => paragraph,
        // Relations / substvars apply only to debian/control; ignore
        // these on debian/copyright so we don't emit spurious edits.
        Deb822Action::DropRelation { .. }
        | Deb822Action::ReplaceRelation { .. }
        | Deb822Action::EnsureRelation { .. }
        | Deb822Action::MoveRelation { .. }
        | Deb822Action::EnsureSubstvar { .. }
        | Deb822Action::DropSubstvar { .. } => return Vec::new(),
        Deb822Action::AppendParagraph { .. } | Deb822Action::ReorderParagraphs { .. } => {
            unreachable!("handled above")
        }
    };

    // RemoveParagraph is structural — strip the paragraph plus its
    // trailing blank line. Mirrors `remove_paragraph_edits` but against
    // the copyright deb822.
    if matches!(action, Deb822Action::RemoveParagraph { .. }) {
        return remove_paragraph_edits_from_deb822(copyright.as_deb822(), selector, original_src);
    }

    // Locate the target paragraph and snapshot its current byte range
    // BEFORE mutation. The range coordinates we report to the editor
    // are against the unmodified buffer.
    let Some(orig_paragraph) = find_paragraph_in_deb822(copyright.as_deb822(), selector) else {
        return Vec::new();
    };
    let paragraph_range = orig_paragraph.text_range();
    let start: usize = paragraph_range.start().into();
    let end: usize = paragraph_range.end().into();
    if end > original_text.len() || start > end {
        return Vec::new();
    }

    // Apply the mutation through the matching typed wrapper so DEP-5
    // field ordering and the License 1-space indent are honoured. Each
    // arm renders the paragraph after mutation and falls through to the
    // common edit-emit at the bottom.
    let mutated = match selector {
        ParagraphSelector::CopyrightHeader => {
            let Some(mut header) = copyright.header() else {
                return Vec::new();
            };
            match apply_typed_copyright_field_op(action, &mut header) {
                Some(true) => header.as_deb822().to_string(),
                _ => return Vec::new(),
            }
        }
        ParagraphSelector::CopyrightFiles { glob } => {
            let Some(mut files) = copyright
                .iter_files()
                .find(|f| f.as_deb822().get("Files").as_deref() == Some(glob.as_str()))
            else {
                return Vec::new();
            };
            match apply_typed_copyright_field_op(action, &mut files) {
                Some(true) => files.as_deb822().to_string(),
                _ => return Vec::new(),
            }
        }
        ParagraphSelector::CopyrightLicense { name: license_name } => {
            let Some(mut license) = copyright.iter_licenses().find(|l| {
                l.as_deb822()
                    .get("License")
                    .and_then(|s| s.split_once('\n').map(|(n, _)| n.to_string()).or(Some(s)))
                    .as_deref()
                    == Some(license_name.as_str())
            }) else {
                return Vec::new();
            };
            match apply_typed_copyright_field_op(action, &mut license) {
                Some(true) => license.as_deb822().to_string(),
                _ => return Vec::new(),
            }
        }
        // No typed wrapper for these selectors against debian/copyright;
        // fall back to silent no-op.
        ParagraphSelector::ByKey { .. }
        | ParagraphSelector::Index { .. }
        | ParagraphSelector::Source
        | ParagraphSelector::Binary { .. } => return Vec::new(),
    };

    if original_text[start..end] == mutated {
        return Vec::new();
    }
    let lsp_range = original_src.text_range_to_lsp_range(paragraph_range);
    vec![TextEdit {
        range: lsp_range,
        new_text: mutated,
    }]
}

/// Trait abstracting the `set_field` / `remove_field` / `get` operations
/// that `Header` / `FilesParagraph` / `LicenseParagraph` share. Each
/// typed wrapper bakes in its own field-ordering and indent rules — this
/// trait lets `apply_typed_copyright_field_op` route through them
/// uniformly. The trait methods are renamed to avoid recursing into the
/// inherent methods on each impl.
trait CopyrightFieldOps {
    fn copyright_set(&mut self, name: &str, value: &str);
    fn copyright_remove(&mut self, name: &str);
    fn copyright_get(&self, name: &str) -> Option<String>;
}

impl CopyrightFieldOps for debian_copyright::lossless::Header {
    fn copyright_set(&mut self, name: &str, value: &str) {
        self.set_field(name, value);
    }
    fn copyright_remove(&mut self, name: &str) {
        self.remove_field(name);
    }
    fn copyright_get(&self, name: &str) -> Option<String> {
        self.as_deb822().get(name)
    }
}

impl CopyrightFieldOps for debian_copyright::lossless::FilesParagraph {
    fn copyright_set(&mut self, name: &str, value: &str) {
        self.set_field(name, value);
    }
    fn copyright_remove(&mut self, name: &str) {
        self.remove_field(name);
    }
    fn copyright_get(&self, name: &str) -> Option<String> {
        self.as_deb822().get(name)
    }
}

impl CopyrightFieldOps for debian_copyright::lossless::LicenseParagraph {
    fn copyright_set(&mut self, name: &str, value: &str) {
        self.set_field(name, value);
    }
    fn copyright_remove(&mut self, name: &str) {
        self.remove_field(name);
    }
    fn copyright_get(&self, name: &str) -> Option<String> {
        self.as_deb822().get(name)
    }
}

/// Run a `Deb822Action` through a typed copyright paragraph's
/// field-ops, returning `Some(true)` when a mutation was applied,
/// `Some(false)` when the mutation was a no-op (and we should emit no
/// edit), and `None` when the action's shape doesn't apply to copyright.
fn apply_typed_copyright_field_op<T: CopyrightFieldOps>(
    action: &Deb822Action,
    target: &mut T,
) -> Option<bool> {
    match action {
        Deb822Action::SetField { field, value, .. }
        | Deb822Action::SetFieldWithIndent { field, value, .. } => {
            // The typed setter already enforces DEP-5 indent rules
            // (License gets 1-space); the action's IndentPattern is
            // ignored on copyright.
            if target.copyright_get(field).as_deref() == Some(value.as_str()) {
                return Some(false);
            }
            target.copyright_set(field, value);
            Some(true)
        }
        Deb822Action::RemoveField { field, .. } => {
            if target.copyright_get(field).is_none() {
                return Some(false);
            }
            target.copyright_remove(field);
            Some(true)
        }
        Deb822Action::RenameField { from, to, .. } => {
            // No typed `rename` on the copyright wrappers; do it as
            // (read + set new + remove old) using only the trait API.
            let value = target.copyright_get(from)?;
            if target.copyright_get(to).is_some() {
                // Refuse to clobber an existing destination field.
                return Some(false);
            }
            target.copyright_set(to, &value);
            target.copyright_remove(from);
            Some(true)
        }
        // No typed equivalent on the copyright wrappers.
        Deb822Action::NormalizeFieldSpacing { .. } => None,
        _ => None,
    }
}

/// Mirror of `remove_paragraph_edits` but against a `&Deb822` instead of
/// a `&Control`. Used only by the copyright path.
fn remove_paragraph_edits_from_deb822(
    deb822: &deb822_lossless::Deb822,
    selector: &ParagraphSelector,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    let Some(paragraph) = find_paragraph_in_deb822(deb822, selector) else {
        return Vec::new();
    };
    let para_range = paragraph.text_range();
    let start: usize = para_range.start().into();
    let end_after_blank = absorb_trailing_blank_line(original_text, para_range.end().into());
    let text_range = rowan::TextRange::new((start as u32).into(), (end_after_blank as u32).into());
    let lsp_range = original_src.text_range_to_lsp_range(text_range);
    vec![TextEdit {
        range: lsp_range,
        new_text: String::new(),
    }]
}

pub(super) fn find_paragraph(
    control: &Control,
    selector: &ParagraphSelector,
) -> Option<deb822_lossless::Paragraph> {
    match selector {
        ParagraphSelector::Source => control.source().map(|s| s.as_deb822().clone()),
        ParagraphSelector::Binary { package } => control
            .binaries()
            .find(|b| b.name().as_deref() == Some(package.as_str()))
            .map(|b| b.as_deb822().clone()),
        ParagraphSelector::Index { index } => control.as_deb822().paragraphs().nth(*index),
        ParagraphSelector::ByKey { field, value } => control
            .as_deb822()
            .paragraphs()
            .find(|p| p.get(field).as_deref() == Some(value.as_str())),
        // Copyright-only selectors don't apply to debian/control.
        ParagraphSelector::CopyrightHeader
        | ParagraphSelector::CopyrightFiles { .. }
        | ParagraphSelector::CopyrightLicense { .. } => None,
    }
}

/// Read-only paragraph lookup against any deb822 file. Used for locating
/// source ranges (squiggle anchors); for mutations on debian/control we
/// go through the typed `Source` / `Binary` accessors so the canonical
/// debian-control field ordering is preserved.
pub(super) fn find_paragraph_in_deb822(
    deb822: &deb822_lossless::Deb822,
    selector: &ParagraphSelector,
) -> Option<deb822_lossless::Paragraph> {
    match selector {
        ParagraphSelector::Source => deb822.paragraphs().find(|p| p.get("Source").is_some()),
        ParagraphSelector::Binary { package } => deb822
            .paragraphs()
            .find(|p| p.get("Package").as_deref() == Some(package.as_str())),
        ParagraphSelector::CopyrightHeader => deb822.paragraphs().next(),
        ParagraphSelector::CopyrightFiles { glob } => deb822
            .paragraphs()
            .find(|p| p.get("Files").as_deref() == Some(glob.as_str())),
        ParagraphSelector::CopyrightLicense { name } => deb822.paragraphs().find(|p| {
            p.get("Files").is_none()
                && !p.contains_key("Format")
                && p.get("License")
                    .and_then(|l| l.split_once('\n').map(|(s, _)| s.to_string()).or(Some(l)))
                    .as_deref()
                    == Some(name.as_str())
        }),
        ParagraphSelector::Index { index } => deb822.paragraphs().nth(*index),
        ParagraphSelector::ByKey { field, value } => deb822
            .paragraphs()
            .find(|p| p.get(field).as_deref() == Some(value.as_str())),
    }
}

pub(super) fn find_entry_in_paragraph(
    paragraph: &deb822_lossless::Paragraph,
    field: &str,
) -> Option<deb822_lossless::Entry> {
    paragraph
        .entries()
        .find(|e| e.key().as_deref() == Some(field))
}

/// Find the byte offset at which to insert a new `field` entry into
/// `paragraph`, respecting the canonical field order for the paragraph type.
///
/// Returns the offset just after the last trailing newline of the entry that
/// should precede the new field, or the start of the first entry that should
/// follow it, or the end of the paragraph if no ordering constraint applies.
fn insert_offset_for_field(
    paragraph: &deb822_lossless::Paragraph,
    field: &str,
    selector: &ParagraphSelector,
) -> usize {
    let field_order: &[&str] = match selector {
        ParagraphSelector::Source => &debian_control::lossless::SOURCE_FIELD_ORDER,
        ParagraphSelector::Binary { .. } => &debian_control::lossless::BINARY_FIELD_ORDER,
        _ => &[],
    };

    if field_order.is_empty() {
        return paragraph.text_range().end().into();
    }

    let new_pos = field_order.iter().position(|f| f.eq_ignore_ascii_case(field));

    // Walk the existing entries and find the last one whose canonical position
    // is before `new_pos`, and the first one whose position is after it.
    let mut insert_after: Option<usize> = None; // byte end of predecessor entry
    let mut insert_before: Option<usize> = None; // byte start of successor entry

    for entry in paragraph.entries() {
        let Some(key) = entry.key() else { continue };
        let existing_pos = field_order
            .iter()
            .position(|f| f.eq_ignore_ascii_case(&key));
        let cmp = match (new_pos, existing_pos) {
            (Some(n), Some(e)) => n.cmp(&e),
            // New field not in order list → goes at end; no successor.
            (None, _) => std::cmp::Ordering::Greater,
            // Existing field not in order list → treat as after new field.
            (Some(_), None) => std::cmp::Ordering::Less,
        };
        match cmp {
            std::cmp::Ordering::Greater => {
                // existing comes before new field
                insert_after = Some(entry.text_range().end().into());
            }
            std::cmp::Ordering::Less => {
                // existing comes after new field; record the earliest such
                if insert_before.map_or(true, |b| entry.text_range().start() < (b as u32).into()) {
                    insert_before = Some(entry.text_range().start().into());
                }
            }
            std::cmp::Ordering::Equal => {} // same field, shouldn't happen
        }
    }

    // Prefer inserting just after the last predecessor; if none, before the
    // first successor; if neither, at the end of the paragraph.
    insert_after
        .or(insert_before)
        .unwrap_or_else(|| paragraph.text_range().end().into())
}

pub(super) fn set_field_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    value: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    if let Some(entry) = find_entry_in_paragraph(&paragraph, field) {
        // Field exists — replace just its value.
        if entry.value().as_str() == value {
            return Vec::new();
        }
        let Some(value_range) = entry.value_range() else {
            return Vec::new();
        };
        let lsp_range = original_src.text_range_to_lsp_range(value_range);
        vec![TextEdit {
            range: lsp_range,
            new_text: value.to_string(),
        }]
    } else {
        let insertion = insert_offset_for_field(&paragraph, field, selector);
        let pos = original_src.offset_to_position((insertion as u32).into());
        vec![TextEdit {
            range: Range { start: pos, end: pos },
            new_text: format!("{}: {}\n", field, value),
        }]
    }
}

fn remove_field_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let Some(entry) = find_entry_in_paragraph(&paragraph, field) else {
        // Field not present; no-op.
        return Vec::new();
    };
    let lsp_range = original_src.text_range_to_lsp_range(entry.text_range());
    vec![TextEdit {
        range: lsp_range,
        new_text: String::new(),
    }]
}

fn remove_paragraph_edits(
    control: &Control,
    selector: &ParagraphSelector,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let para_range = paragraph.text_range();
    let start: usize = para_range.start().into();
    let end_after_blank = absorb_trailing_blank_line(original_text, para_range.end().into());
    let text_range = rowan::TextRange::new((start as u32).into(), (end_after_blank as u32).into());
    let lsp_range = original_src.text_range_to_lsp_range(text_range);
    vec![TextEdit {
        range: lsp_range,
        new_text: String::new(),
    }]
}

/// Extend `end` past the blank line that follows the paragraph (if any),
/// so that removing `[start..returned_end]` leaves the surrounding text
/// looking like a clean paragraph boundary instead of a stray empty line.
pub(super) fn absorb_trailing_blank_line(text: &str, end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = end;
    // Skip horizontal whitespace, then exactly one newline.
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

pub(super) fn append_paragraph_edits(
    fields: &[(String, String)],
    indent: Option<usize>,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    if fields.is_empty() {
        return Vec::new();
    }
    let rendered = render_paragraph(fields, indent);
    // Insert at end-of-file. Prefix a blank-line separator unless the
    // file is empty or already ends with one.
    let needs_separator = !ends_with_blank_line(original_text);
    let insertion = if needs_separator {
        format!("\n{}", rendered)
    } else {
        rendered
    };
    let pos = original_src.offset_to_position((original_text.len() as u32).into());
    vec![TextEdit {
        range: Range {
            start: pos,
            end: pos,
        },
        new_text: insertion,
    }]
}

/// Render `(field, value)` pairs as a deb822 paragraph followed by a
/// trailing newline. Multi-line values get their continuation lines
/// indented by `indent` spaces (the deb822 default is single-space).
fn render_paragraph(fields: &[(String, String)], indent: Option<usize>) -> String {
    let pad = " ".repeat(indent.unwrap_or(1));
    let mut out = String::new();
    for (k, v) in fields {
        out.push_str(k);
        out.push_str(": ");
        let mut lines = v.split('\n');
        if let Some(first) = lines.next() {
            out.push_str(first);
            out.push('\n');
        }
        for line in lines {
            out.push_str(&pad);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn ends_with_blank_line(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    text.ends_with("\n\n") || text.ends_with("\r\n\r\n")
}

fn rename_field_edits(
    control: &Control,
    selector: &ParagraphSelector,
    from: &str,
    to: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let Some(entry) = find_entry_in_paragraph(&paragraph, from) else {
        return Vec::new();
    };
    let Some(key_range) = entry.key_range() else {
        return Vec::new();
    };
    let lsp_range = original_src.text_range_to_lsp_range(key_range);
    vec![TextEdit {
        range: lsp_range,
        new_text: to.to_string(),
    }]
}

/// Replace the whitespace between `:` and the field's value with exactly
/// one space. For empty values (e.g. `Field:  \n`), strip the whitespace
/// entirely so the line becomes `Field:\n`. Mirrors the canonical form
/// produced by `Entry::normalize_field_spacing`.
fn normalize_field_spacing_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let Some(entry) = find_entry_in_paragraph(&paragraph, field) else {
        return Vec::new();
    };
    let Some(colon) = entry.colon_range() else {
        return Vec::new();
    };

    let gap_start: usize = colon.end().into();
    let entry_end: usize = entry.text_range().end().into();

    // Determine the desired replacement: " " when there is value content
    // on the same line as the colon, "" when the line is empty or all
    // whitespace through the newline.
    let (gap_end, replacement) = match entry.value_range() {
        Some(value_range) => {
            let value_start: usize = value_range.start().into();
            // If the value sits on a continuation line, the colon is
            // immediately followed by `\n` and we don't touch it.
            if original_text.as_bytes().get(gap_start) == Some(&b'\n') {
                return Vec::new();
            }
            (value_start, " ")
        }
        None => {
            // Empty value: strip whitespace up to (but not including)
            // the newline at the end of the line.
            let mut i = gap_start;
            let bytes = original_text.as_bytes();
            while i < entry_end && (bytes.get(i) == Some(&b' ') || bytes.get(i) == Some(&b'\t')) {
                i += 1;
            }
            (i, "")
        }
    };

    if gap_end <= gap_start {
        return Vec::new();
    }
    let current = &original_text[gap_start..gap_end];
    if current == replacement {
        return Vec::new();
    }
    let lsp_range = original_src.text_range_to_lsp_range(rowan::TextRange::new(
        (gap_start as u32).into(),
        (gap_end as u32).into(),
    ));
    vec![TextEdit {
        range: lsp_range,
        new_text: replacement.to_string(),
    }]
}

/// Apply a relations-field mutation in memory and emit a `TextEdit` over
/// the field's value range with the new rendered value. `mutate` is the
/// piece that does the actual edit on a parsed `Relations`; it returns
/// `true` if anything changed.
fn relations_field_edits<F>(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    original_src: crate::position::Source<'_>,
    mutate: F,
) -> Vec<TextEdit>
where
    F: FnOnce(&mut debian_control::lossless::Relations) -> bool,
{
    let original_text = original_src.text;
    use debian_control::lossless::Relations;

    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let Some(entry) = find_entry_in_paragraph(&paragraph, field) else {
        return Vec::new();
    };
    let Some(value_range) = entry.value_range() else {
        return Vec::new();
    };
    let value_start: usize = value_range.start().into();
    let value_end: usize = value_range.end().into();
    if value_end > original_text.len() || value_start > value_end {
        return Vec::new();
    }
    let value_text = &original_text[value_start..value_end];

    // Substvars are valid in Build-Depends/Depends/etc.; allow them so a
    // pre-existing `${misc:Depends}` doesn't trip a "syntax error" exit.
    let (mut relations, _errors) = Relations::parse_relaxed(value_text, true);
    if !mutate(&mut relations) {
        return Vec::new();
    }
    let new_value = relations.to_string();
    if new_value == value_text {
        return Vec::new();
    }
    let lsp_range = original_src.text_range_to_lsp_range(value_range);
    vec![TextEdit {
        range: lsp_range,
        new_text: new_value,
    }]
}

fn drop_relation_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    package: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    relations_field_edits(control, selector, field, original_src, |relations| {
        // Walk relations in reverse so popping by index leaves earlier
        // indices stable. `iter_relations_for` returns `(idx, entry)`.
        let indices: Vec<usize> = relations
            .iter_relations_for(package)
            .map(|(idx, _)| idx)
            .collect();
        if indices.is_empty() {
            return false;
        }
        for idx in indices.into_iter().rev() {
            relations.remove_entry(idx);
        }
        true
    })
}

fn ensure_substvar_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    substvar: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    if find_entry_in_paragraph(&paragraph, field).is_none() {
        // Field absent — insert it containing just the substvar, respecting
        // canonical field ordering for the paragraph type.
        return set_field_edits(control, selector, field, substvar, original_src);
    }
    relations_field_edits(control, selector, field, original_src, |relations| {
        if relations.substvars().any(|s| s.trim() == substvar.trim()) {
            return false;
        }
        relations.ensure_substvar(substvar).is_ok()
    })
}

fn drop_substvar_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    substvar: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    relations_field_edits(control, selector, field, original_src, |relations| {
        if !relations.substvars().any(|s| s.trim() == substvar.trim()) {
            return false;
        }
        relations.drop_substvar(substvar);
        true
    })
}

/// `Deb822Action::SetFieldWithIndent`: set or insert a field, applying the
/// requested continuation-line indent pattern to multi-line values. For the
/// simple case where the field exists *and* the new value has no embedded
/// newline, this is identical to `set_field_edits`. Otherwise we need to
/// rewrite the entire field entry so the indent gets applied.
fn set_field_with_indent_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    value: &str,
    indent: &IndentPattern,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let pad = match indent {
        IndentPattern::Fixed { spaces } => " ".repeat(*spaces),
        IndentPattern::FieldNameLength => " ".repeat(field.len() + 2),
    };
    let rendered = render_field_with_indent(field, value, &pad);
    if let Some(entry) = find_entry_in_paragraph(&paragraph, field) {
        if entry.value().as_str() == value {
            return Vec::new();
        }
        let lsp_range = original_src.text_range_to_lsp_range(entry.text_range());
        vec![TextEdit {
            range: lsp_range,
            new_text: rendered,
        }]
    } else {
        let para_range = paragraph.text_range();
        let insertion: usize = para_range.end().into();
        let pos = original_src.offset_to_position((insertion as u32).into());
        vec![TextEdit {
            range: Range {
                start: pos,
                end: pos,
            },
            new_text: rendered,
        }]
    }
}

/// Render a deb822 field entry with continuation-line indent `pad`. The
/// first line uses `Field: value`; subsequent lines are prefixed with `pad`.
/// Always emits a trailing newline.
fn render_field_with_indent(field: &str, value: &str, pad: &str) -> String {
    let mut out = String::new();
    let mut lines = value.split('\n');
    out.push_str(field);
    out.push_str(": ");
    if let Some(first) = lines.next() {
        out.push_str(first);
        out.push('\n');
    }
    for line in lines {
        out.push_str(pad);
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `Deb822Action::ReplaceRelation`: replace the first relation naming
/// `from_package` with `to_entry`. If `to_entry`'s package is already
/// elsewhere in the field, drop the original instead. Mirrors the
/// applier's `replace_relation_in_paragraph`.
fn replace_relation_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    from_package: &str,
    to_entry: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    use debian_control::lossless::relations::Entry;
    use std::str::FromStr;

    relations_field_edits(control, selector, field, original_src, |relations| {
        let Some((idx, _)) = relations.iter_relations_for(from_package).next() else {
            return false;
        };
        let Ok(new_entry) = Entry::from_str(to_entry) else {
            return false;
        };
        let new_name = new_entry
            .relations()
            .next()
            .and_then(|r| r.try_name())
            .unwrap_or_default();
        let new_already_present = !new_name.is_empty()
            && relations
                .iter_relations_for(&new_name)
                .any(|(other_idx, _)| other_idx != idx);
        if new_already_present {
            relations.drop_dependency(from_package);
        } else {
            relations.replace(idx, new_entry);
        }
        true
    })
}

/// `Deb822Action::EnsureRelation`: ensure a relation entry is present in a
/// relations field. Falls through to the same code paths
/// debian-analyzer's `ensure_some_version` / `ensure_minimum_version` /
/// `ensure_exact_version` use, mirroring the applier.
///
/// If the field doesn't exist yet, fall back to `set_field_edits` to insert
/// it with the literal `entry` text.
fn ensure_relation_edits(
    control: &Control,
    selector: &ParagraphSelector,
    field: &str,
    entry: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    if find_entry_in_paragraph(&paragraph, field).is_none() {
        // Field absent — insert it with the literal entry text.
        return set_field_edits(control, selector, field, entry, original_src);
    }

    relations_field_edits(control, selector, field, original_src, |relations| {
        use debian_control::lossless::Entry;
        use debian_control::relations::VersionConstraint;
        use std::str::FromStr;

        let Ok(requested_entry) = Entry::from_str(entry) else {
            return false;
        };
        let Some(first) = requested_entry.relations().next() else {
            return false;
        };
        let Some(name) = first.try_name() else {
            return false;
        };
        let before = relations.to_string();
        match first.version() {
            Some((VersionConstraint::Equal, ver)) => {
                relations.ensure_exact_version(&name, &ver);
            }
            Some((VersionConstraint::GreaterThanEqual, ver)) => {
                relations.ensure_minimum_version(&name, &ver);
            }
            Some(_) => return false,
            None => {
                relations.ensure_some_version(&name);
            }
        }
        relations.to_string() != before
    })
}

/// `Deb822Action::MoveRelation`: move the named entry from `from_field` to
/// `to_field`, emitting one TextEdit per affected field. Mirrors
/// `move_relation_in_paragraph`. Reuses `relations_field_edits` for each
/// side; if the source field becomes empty we replace the whole entry
/// with the empty string.
fn move_relation_edits(
    control: &Control,
    selector: &ParagraphSelector,
    from_field: &str,
    to_field: &str,
    package: &str,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    use debian_control::lossless::Relations;

    let Some(paragraph) = find_paragraph(control, selector) else {
        return Vec::new();
    };
    let Some(from_entry) = find_entry_in_paragraph(&paragraph, from_field) else {
        return Vec::new();
    };
    let Some(from_value_range) = from_entry.value_range() else {
        return Vec::new();
    };
    let from_start: usize = from_value_range.start().into();
    let from_end: usize = from_value_range.end().into();
    if from_end > original_text.len() || from_start > from_end {
        return Vec::new();
    }
    let from_text = &original_text[from_start..from_end];
    let (mut from_relations, _) = Relations::parse_relaxed(from_text, true);
    let Ok((_pos, moved_entry)) = from_relations.get_relation(package) else {
        return Vec::new();
    };
    if !from_relations.drop_dependency(package) {
        return Vec::new();
    }

    let mut edits = Vec::new();

    // Source field: either drop it entirely (when empty) or replace its
    // value with the new rendering.
    if from_relations.is_empty() || from_relations.to_string().trim().is_empty() {
        let lsp_range = original_src.text_range_to_lsp_range(from_entry.text_range());
        edits.push(TextEdit {
            range: lsp_range,
            new_text: String::new(),
        });
    } else {
        let new_value = from_relations.to_string();
        if new_value != from_text {
            let lsp_range = original_src.text_range_to_lsp_range(from_value_range);
            edits.push(TextEdit {
                range: lsp_range,
                new_text: new_value,
            });
        }
    }

    // Destination field: insert if missing, otherwise rewrite its value
    // with the moved entry appended.
    if let Some(to_entry) = find_entry_in_paragraph(&paragraph, to_field) {
        let Some(to_value_range) = to_entry.value_range() else {
            return edits;
        };
        let to_start: usize = to_value_range.start().into();
        let to_end: usize = to_value_range.end().into();
        if to_end > original_text.len() || to_start > to_end {
            return edits;
        }
        let to_text = &original_text[to_start..to_end];
        let (mut to_relations, _) = Relations::parse_relaxed(to_text, true);
        to_relations.add_dependency(moved_entry, None);
        let new_value = to_relations.to_string();
        if new_value != to_text {
            let lsp_range = original_src.text_range_to_lsp_range(to_value_range);
            edits.push(TextEdit {
                range: lsp_range,
                new_text: new_value,
            });
        }
    } else {
        // Append a new field at end of paragraph with just the moved entry.
        let para_range = paragraph.text_range();
        let insertion: usize = para_range.end().into();
        let pos = original_src.offset_to_position((insertion as u32).into());
        let new_text = format!("{}: {}\n", to_field, moved_entry);
        edits.push(TextEdit {
            range: Range {
                start: pos,
                end: pos,
            },
            new_text,
        });
    }

    edits
}

/// `Deb822Action::ReorderParagraphs`: pull out paragraphs whose `key_field`
/// values appear in `order` and re-insert them at their original positions
/// in the order specified. Emits one TextEdit per moved paragraph: the
/// participating slots stay where they are, and we just rewrite the bytes
/// in each slot with a different paragraph's text.
fn reorder_paragraphs_edits(
    control: &Control,
    key_field: &str,
    order: &[String],
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    let original_text = original_src.text;
    // Collect (index, slot range, current text, key value) for every
    // paragraph that has key_field.
    let participants: Vec<(usize, rowan::TextRange, String, String)> = control
        .as_deb822()
        .paragraphs()
        .enumerate()
        .filter_map(|(idx, p)| {
            let key = p.get(key_field)?.to_string();
            let range = p.text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            if end > original_text.len() || start > end {
                return None;
            }
            let text = original_text[start..end].to_string();
            Some((idx, range, text, key))
        })
        .collect();

    if participants.is_empty() {
        return Vec::new();
    }

    // Build the desired key sequence, restricted to keys present.
    let present: std::collections::HashSet<&str> =
        participants.iter().map(|(_, _, _, k)| k.as_str()).collect();
    let desired: Vec<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|k| present.contains(k))
        .collect();
    if desired.len() != participants.len() {
        // Some participating paragraphs aren't covered by `order`. Treat
        // as a no-op, mirroring the applier (`reorder_paragraphs`).
        return Vec::new();
    }

    // For each slot, find the paragraph whose key matches the desired
    // order at that position, and emit a TextEdit replacing the slot's
    // current text with that paragraph's text.
    let by_key: HashMap<&str, &str> = participants
        .iter()
        .map(|(_, _, text, key)| (key.as_str(), text.as_str()))
        .collect();

    let mut edits = Vec::new();
    let mut any_change = false;
    for ((_idx, range, current_text, _current_key), desired_key) in
        participants.iter().zip(desired.iter())
    {
        let Some(new_text) = by_key.get(desired_key) else {
            continue;
        };
        if *new_text == current_text.as_str() {
            continue;
        }
        any_change = true;
        let lsp_range = original_src.text_range_to_lsp_range(*range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: (*new_text).to_string(),
        });
    }

    if !any_change {
        Vec::new()
    } else {
        edits
    }
}
