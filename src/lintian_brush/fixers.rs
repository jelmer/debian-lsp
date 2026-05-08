//! Glue between debian-lsp's `code_action` handler and lintian-brush's
//! detector registry.
//!
//! Detectors live in `lintian_brush::workspace::iter_detector_registrations()`.
//! Each one takes our [`LspDebianWorkspace`] and returns
//! [`lintian_brush::diagnostic::Diagnostic`]s carrying serialisable
//! [`lintian_brush::diagnostic::Action`]s. We translate the actions into
//! LSP `TextEdit`s and surface each diagnostic as a `CodeAction`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ::lintian_brush::diagnostic::{
    Action, ActionPlan, ChangelogAction, Deb822Action, Dep3Action, Diagnostic as LbDiagnostic,
    FilesystemAction, IndentPattern, ParagraphSelector, WatchAction, YamlAction, YamlPathComponent,
};
use ::lintian_brush::workspace::{
    iter_detector_registrations, ChangelogAspect, DetectorCost, Trigger, WatchAspect,
};
use ::lintian_brush::{FixerPreferences, Version};
use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, DeleteFile, Diagnostic,
    DocumentChangeOperation, DocumentChanges, NumberOrString, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, RenameFile, ResourceOp,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};

use crate::lintian_brush::workspace::LspDebianWorkspace;
use crate::workspace::{SourceFile, Workspace};
use crate::FileInfo;

pub use crate::phase::RunPhase;

/// Maximum [`DetectorCost`] this phase will run.
fn phase_max_cost(phase: RunPhase) -> DetectorCost {
    match phase {
        RunPhase::Keystroke => DetectorCost::Filesystem,
        RunPhase::Open => DetectorCost::Subprocess,
        RunPhase::Explicit => DetectorCost::Network,
    }
}

/// Whether detectors are allowed to use the network in this phase.
fn phase_allow_net(phase: RunPhase) -> bool {
    matches!(phase, RunPhase::Explicit)
}

/// Pre-computed change context, built once per detector-running
/// call so each detector's triggers can be checked without re-walking
/// the rowan tree.
struct ChangeContext<'a> {
    /// Set of (paragraph_key, field) pairs whose entries overlap any
    /// changed range. `None` means we don't have field-level info —
    /// triggers fall back to file-level match (whole-file scope).
    deb822: Option<Deb822ChangeIndex>,
    changelog: Option<&'a ChangeLog>,
    yaml: Option<&'a yaml_edit::YamlFile>,
    watch: Option<&'a debian_watch::parse::ParsedWatchFile>,
    changed_ranges: Option<&'a [rowan::TextRange]>,
}

/// Decide whether a detector with the given `triggers` should run
/// for an edit on `rel`.
///
/// Takes the static triggers slice off `DetectorRegistration` so we
/// can decide *before* instantiating the detector — keeps us from
/// allocating ~150 `Box<dyn Detector>` values per keystroke just to
/// throw most of them away.
///
/// Filtering is conservative: a detector with no declared triggers
/// always runs, and a detector with at least one trigger that *might*
/// match runs even if other triggers definitely don't. We never block
/// a detector that the registry hasn't told us is irrelevant.
fn triggers_match(triggers: &'static [Trigger], rel: &Path, ctx: &ChangeContext<'_>) -> bool {
    if triggers.is_empty() {
        // No declared triggers — be conservative and run it.
        return true;
    }
    triggers.iter().any(|t| trigger_matches(t, rel, ctx))
}

/// Does `trigger` match an edit on `rel`, given the pre-built
/// `ChangeContext`?  When the relevant parse is `None` we fall back
/// to the file-level match (over-trigger rather than miss).
fn trigger_matches(trigger: &Trigger, rel: &Path, ctx: &ChangeContext<'_>) -> bool {
    match trigger {
        Trigger::File(p) => Path::new(p) == rel,
        Trigger::Glob(g) => glob_matches(g, rel),
        Trigger::Deb822Field {
            file,
            paragraph_key,
            field,
        } => {
            if Path::new(file) != rel {
                return false;
            }
            let Some(index) = &ctx.deb822 else {
                // No index built — either no parse, or no changed-range
                // info to narrow on. Assume possible match.
                return true;
            };
            index.matches(paragraph_key, field)
        }
        Trigger::Watch(aspect) => {
            if rel != Path::new("debian/watch") {
                return false;
            }
            let Some(watch) = ctx.watch else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| watch_range_touches_aspect(watch, *r, *aspect))
        }
        Trigger::Changelog(aspect) => {
            if rel != Path::new("debian/changelog") {
                return false;
            }
            let Some(changelog) = ctx.changelog else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| changelog_range_touches_aspect(changelog, *r, *aspect))
        }
        Trigger::UpstreamMetadataField(field) => {
            if rel != Path::new("debian/upstream/metadata") {
                return false;
            }
            let Some(yaml) = ctx.yaml else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| yaml_range_touches_top_field(yaml, *r, field))
        }
    }
}

/// Match an `fnmatch(3)`-style glob against `rel`. Supports `*` (any
/// run of non-separator chars) and `?` (single non-separator char);
/// no character classes, no `**`. Path separators are matched literally
/// — a `*` does not cross a `/`.
fn glob_matches(pattern: &str, rel: &Path) -> bool {
    let Some(s) = rel.to_str() else {
        return false;
    };
    glob_matches_str(pattern, s)
}

fn glob_matches_str(pattern: &str, s: &str) -> bool {
    let pat = pattern.as_bytes();
    let txt = s.as_bytes();
    glob_matches_inner(pat, 0, txt, 0)
}

fn glob_matches_inner(pat: &[u8], mut pi: usize, txt: &[u8], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            b'*' => {
                // Skip runs of `*`.
                while pi < pat.len() && pat[pi] == b'*' {
                    pi += 1;
                }
                if pi == pat.len() {
                    // Trailing `*` — match anything but a `/`.
                    return !txt[ti..].contains(&b'/');
                }
                // Try every position in the current path component.
                while ti <= txt.len() {
                    if glob_matches_inner(pat, pi, txt, ti) {
                        return true;
                    }
                    if ti == txt.len() || txt[ti] == b'/' {
                        return false;
                    }
                    ti += 1;
                }
                return false;
            }
            b'?' => {
                if ti >= txt.len() || txt[ti] == b'/' {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            c => {
                if ti >= txt.len() || txt[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == txt.len()
}

/// All `(paragraph_key, field_name)` pairs whose entries overlap any
/// of the changed ranges in a deb822 file. Built once per
/// detector-running call so each detector's `Trigger::Deb822Field`
/// triggers can be checked against a pre-walked set instead of
/// re-walking the rowan tree per trigger.
///
/// The cardinality is bounded by the number of (paragraph,
/// field) entries actually touched — for a single keystroke this is
/// typically 1, almost never more than a handful. Stores the
/// *identifying* keys present in each touched paragraph (so a
/// `Trigger::Deb822Field { paragraph_key: "Source", .. }` matches
/// any paragraph whose touched entries include `Source`).
#[derive(Default)]
struct Deb822ChangeIndex {
    /// Pairs of (paragraph-identifying-key, touched-field-name).
    /// Both come from actual entry keys in the deb822 file, so they
    /// can be matched against trigger patterns (which support `*`
    /// wildcards) by walking this set once per trigger.
    entries: Vec<(String, String)>,
}

impl Deb822ChangeIndex {
    fn build(deb822: &deb822_lossless::Deb822, ranges: &[rowan::TextRange]) -> Self {
        let mut entries: Vec<(String, String)> = Vec::new();
        for &range in ranges {
            for paragraph in deb822.paragraphs_in_range(range) {
                // Snapshot every key in the paragraph — these are the
                // candidate `paragraph_key` values that select this
                // paragraph as the trigger scope.
                let para_keys: Vec<String> = paragraph.entries().filter_map(|e| e.key()).collect();
                if para_keys.is_empty() {
                    continue;
                }
                for entry in paragraph.entries_in_range(range) {
                    let Some(touched_field) = entry.key() else {
                        continue;
                    };
                    for para_key in &para_keys {
                        let pair = (para_key.clone(), touched_field.clone());
                        if !entries.contains(&pair) {
                            entries.push(pair);
                        }
                    }
                }
            }
        }
        Deb822ChangeIndex { entries }
    }

    fn matches(&self, paragraph_key: &str, field: &str) -> bool {
        self.entries
            .iter()
            .any(|(pk, f)| name_matches(paragraph_key, pk) && name_matches(field, f))
    }
}

/// Does the changed `range` touch any part of a changelog entry that
/// corresponds to `aspect`? Conservative: when in doubt, return true.
fn changelog_range_touches_aspect(
    changelog: &ChangeLog,
    range: rowan::TextRange,
    aspect: ChangelogAspect,
) -> bool {
    use rowan::ast::AstNode as _;
    for entry in changelog.entries_in_range(range) {
        let entry_range = entry.syntax().text_range();
        if !ranges_overlap(entry_range, range) {
            continue;
        }
        // Determine whether the changed range falls in the header,
        // body, or footer. Without finer-grained API, treat any
        // changelog edit as potentially touching the aspect — but we
        // can rule out body-only edits against header-aspect triggers.
        match aspect {
            ChangelogAspect::Body => {
                // Any overlap with an entry plausibly touches the body.
                return true;
            }
            ChangelogAspect::Version | ChangelogAspect::Distribution | ChangelogAspect::Urgency => {
                if let Some(header) = entry.header() {
                    if ranges_overlap(header.syntax().text_range(), range) {
                        return true;
                    }
                }
            }
            ChangelogAspect::Maintainer | ChangelogAspect::Timestamp => {
                if let Some(footer) = entry.footer() {
                    if ranges_overlap(footer.syntax().text_range(), range) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Does the changed `range` overlap any part of the watch file that
/// corresponds to `aspect`? Conservative: when in doubt, return true.
fn watch_range_touches_aspect(
    watch: &debian_watch::parse::ParsedWatchFile,
    range: rowan::TextRange,
    aspect: WatchAspect,
) -> bool {
    match aspect {
        WatchAspect::Version => watch
            .version_range()
            .map(|r| ranges_overlap(r, range))
            .unwrap_or(false),
        WatchAspect::Source => watch.entries().any(|e| {
            e.url_range()
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
        WatchAspect::MatchingPattern => watch.entries().any(|e| {
            e.matching_pattern_range()
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
        WatchAspect::Template(kind) => watch.entries().any(|e| {
            let Some(template_range) = e.template_range() else {
                return false;
            };
            if !ranges_overlap(template_range, range) {
                return false;
            }
            // A bare "*" matches any kind.
            if kind == "*" {
                return true;
            }
            e.template_kind().as_deref() == Some(kind)
        }),
        WatchAspect::Option(name) => watch.entries().any(|e| {
            e.option_range(name)
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
    }
}

/// Does the changed `range` touch a top-level mapping entry in `yaml`
/// whose key matches `field`? `field` follows Trigger's wildcard rules.
fn yaml_range_touches_top_field(
    yaml: &yaml_edit::YamlFile,
    range: rowan::TextRange,
    field: &str,
) -> bool {
    use rowan::ast::AstNode as _;
    let Some(doc) = yaml.document() else {
        return false;
    };
    let Some(mapping) = doc.as_mapping() else {
        return false;
    };
    for entry in mapping.entries() {
        let entry_range = entry.syntax().text_range();
        if !ranges_overlap(entry_range, range) {
            continue;
        }
        let Some(key_node) = entry.key_node() else {
            continue;
        };
        let key = match &key_node {
            yaml_edit::YamlNode::Scalar(s) => s.as_string(),
            _ => continue,
        };
        if name_matches(field, &key) {
            return true;
        }
    }
    false
}

/// Wildcard match for trigger field-/paragraph-key patterns: a bare
/// `*` matches anything, a trailing `*` is a prefix match, otherwise
/// equality.
fn name_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    pattern == name
}

fn ranges_overlap(a: rowan::TextRange, b: rowan::TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
}

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

            let lb_range = diagnostic_range(&diag, &ws, &rel, original_src);

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

/// Pull the salsa-cached deb822 parse for `rel` if `rel` looks like a
/// deb822 file we can extract a `Deb822` from. Used by the trigger
/// filter to narrow `Deb822Field` triggers to fields whose ranges
/// overlap the changed range. Returns `None` for non-deb822 files.
fn parse_for_trigger_filtering_deb822(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<deb822_lossless::Deb822> {
    if rel == Path::new("debian/copyright") {
        ws.parsed_copyright_for(rel).map(|c| c.as_deb822().clone())
    } else {
        ws.parsed_control_for(rel).map(|c| c.as_deb822().clone())
    }
}

fn parse_for_trigger_filtering_changelog(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<ChangeLog> {
    if rel == Path::new("debian/changelog") {
        ws.parsed_changelog_for(rel)
    } else {
        None
    }
}

fn parse_for_trigger_filtering_yaml(
    ws: &LspDebianWorkspace<'_>,
    rel: &Path,
) -> Option<yaml_edit::YamlFile> {
    if rel == Path::new("debian/upstream/metadata") {
        ws.parsed_yaml_for(rel)
    } else {
        None
    }
}

fn parse_for_trigger_filtering_watch(
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
fn diag_touches_file(diag: &LbDiagnostic, rel: &Path) -> bool {
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
fn diagnostic_range(
    diag: &LbDiagnostic,
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
fn locate_action_target(
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
        // Action kinds not yet wired into the LSP translator. These are
        // filtered out of `is_action_translatable` so they shouldn't
        // reach this code path in practice, but we keep the arm
        // exhaustive to prevent silent fall-through if upstream adds new
        // variants.
        Action::Systemd(_)
        | Action::DesktopIni(_)
        | Action::Makefile(_)
        | Action::LintianOverrides(_)
        | Action::Maintscript(_)
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
fn plan_to_workspace_edit(plan: &ActionPlan, ws: &LspDebianWorkspace<'_>) -> Option<WorkspaceEdit> {
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
fn is_action_translatable(action: &Action) -> bool {
    match action {
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
        // TODO: debian/rules *is* cached by the salsa workspace, so the
        // translator could be extended to surface Makefile actions as
        // TextEdits. Filtered out for now so the user doesn't see code
        // actions whose dispatcher would `unimplemented!()`.
        Action::Makefile(_) => false,
        // Line-oriented files; no salsa parse, but the open-buffer text
        // is available. Not yet wired in.
        Action::LintianOverrides(_) | Action::Maintscript(_) => false,
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
        // TODO: debian/rules *is* cacheable in the salsa workspace
        // (`get_parsed_rules`). Wiring it to byte-precise TextEdits is a
        // separate change; until then the detector output is filtered
        // out by `is_action_translatable`.
        Action::Makefile(_) => {
            unimplemented!("Makefile actions not yet wired into the LSP translator")
        }
        Action::LintianOverrides(_) => {
            unimplemented!("LintianOverrides actions not yet wired into the LSP translator")
        }
        Action::Maintscript(_) => {
            unimplemented!("Maintscript actions not yet wired into the LSP translator")
        }
        Action::Debcargo(_) => {
            unimplemented!("Debcargo actions not yet wired into the LSP translator")
        }
        Action::RunCommand(_) => {
            unimplemented!("RunCommand actions have no LSP equivalent")
        }
    }
}

fn changelog_action_to_text_edits(
    action: &ChangelogAction,
    changelog: &ChangeLog,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        ChangelogAction::SetEntryDate {
            version, rfc2822, ..
        } => {
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let Some(timestamp) = entry.timestamp_node() else {
                return Vec::new();
            };
            if entry.timestamp().as_deref() == Some(rfc2822.as_str()) {
                return Vec::new();
            }
            let lsp_range = original_src.text_range_to_lsp_range(timestamp.syntax().text_range());
            vec![TextEdit {
                range: lsp_range,
                new_text: rfc2822.clone(),
            }]
        }
        ChangelogAction::ReplaceEntryChanges { version, lines, .. } => {
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let current: Vec<String> = entry.change_lines().collect();
            if current == *lines {
                return Vec::new();
            }
            let Some(range) = entry_change_block_range(&entry) else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: render_changelog_change_block(lines),
            }]
        }
        ChangelogAction::RemoveBullet {
            version,
            author,
            text,
            occurrence,
            ..
        } => {
            let Some(range) =
                find_bullet_range(changelog, original_src, version, author, text, *occurrence)
            else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: String::new(),
            }]
        }
        ChangelogAction::ReplaceBullet {
            version,
            author,
            text,
            occurrence,
            new_lines,
            ..
        } => {
            if text == &new_lines.join("\n") {
                return Vec::new();
            }
            let Some(range) =
                find_bullet_range(changelog, original_src, version, author, text, *occurrence)
            else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: render_bullet_block(new_lines),
            }]
        }
        ChangelogAction::SetEntryVersion {
            version,
            new_version,
            ..
        } => {
            if version == new_version {
                return Vec::new();
            }
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let Some(range) = entry_version_token_range(&entry) else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: format!("({})", new_version),
            }]
        }
    }
}

/// Locate the byte range of an entry's `(version)` token in the changelog
/// header (e.g. `(2.6.0-1)`). Returns `None` if the entry has no version
/// token (a malformed header).
fn entry_version_token_range(entry: &debian_changelog::Entry) -> Option<rowan::TextRange> {
    use debian_changelog::SyntaxKind;
    let header = entry.header()?;
    header
        .syntax()
        .children_with_tokens()
        .find(|tok| tok.kind() == SyntaxKind::VERSION)
        .map(|tok| tok.text_range())
}

fn changelog_action_range(
    action: &ChangelogAction,
    changelog: &ChangeLog,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    match action {
        ChangelogAction::SetEntryDate { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let ts = entry.timestamp_node()?;
            Some(anchor_src.text_range_to_lsp_range(ts.syntax().text_range()))
        }
        ChangelogAction::ReplaceEntryChanges { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let range = entry_change_block_range(&entry)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
        ChangelogAction::RemoveBullet {
            version,
            author,
            text,
            occurrence,
            ..
        }
        | ChangelogAction::ReplaceBullet {
            version,
            author,
            text,
            occurrence,
            ..
        } => {
            let range =
                find_bullet_range(changelog, anchor_src, version, author, text, *occurrence)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
        ChangelogAction::SetEntryVersion { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let range = entry_version_token_range(&entry)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
    }
}

fn find_changelog_entry(changelog: &ChangeLog, version: &str) -> Option<debian_changelog::Entry> {
    changelog.iter().find(|e| {
        e.version()
            .map(|v| v.to_string() == version)
            .unwrap_or(false)
    })
}

/// Compute the rowan byte range covering all `EntryBody` children of an
/// entry — i.e. the change-lines block. Spans from the first `EntryBody`
/// to the last, picking up any non-body siblings (empty-line separators)
/// that sit between them, and excluding the surrounding header/footer.
fn entry_change_block_range(entry: &debian_changelog::Entry) -> Option<rowan::TextRange> {
    use debian_changelog::EntryBody;
    let bodies: Vec<_> = entry
        .syntax()
        .children()
        .filter_map(EntryBody::cast)
        .collect();
    let first = bodies.first()?;
    let last = bodies.last()?;
    Some(rowan::TextRange::new(
        first.syntax().text_range().start(),
        last.syntax().text_range().end(),
    ))
}

/// Render a list of change-line strings as the textual block they replace.
/// Each line is emitted verbatim, with a trailing newline. An empty `lines`
/// slice produces an empty string.
fn render_changelog_change_block(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Find the bullet matching `(version, author, text, occurrence)` and
/// return its byte range in the file. Walks the same iteration order
/// `apply_changelog_group` uses, then derives the range from the
/// `Change`'s reported line numbers (relative to the parent entry).
fn find_bullet_range(
    changelog: &ChangeLog,
    original_src: crate::position::Source<'_>,
    version: &str,
    author: &Option<String>,
    text: &str,
    occurrence: usize,
) -> Option<rowan::TextRange> {
    use debian_changelog::iter_changes_by_author;
    let mut seen = 0usize;
    for change in iter_changes_by_author(changelog) {
        if change.version().map(|v| v.to_string()).as_deref() != Some(version) {
            continue;
        }
        for bullet in change.split_into_bullets() {
            let bullet_author = bullet.author().map(|s| s.to_string());
            let bullet_text = bullet.lines().join("\n");
            if bullet_author == *author && bullet_text == *text {
                if seen == occurrence {
                    return bullet_byte_range(&bullet, original_src);
                }
                seen += 1;
            }
        }
    }
    None
}

/// Compute the byte range covering a bullet's lines in the source file.
/// Uses the bullet's reported start line (file-relative, 0-indexed) plus
/// the count of lines it occupies, walking `original_src` to map back to
/// byte offsets. Each bullet line is removed in full — leading indent
/// through trailing newline.
fn bullet_byte_range(
    bullet: &debian_changelog::Change,
    original_src: crate::position::Source<'_>,
) -> Option<rowan::TextRange> {
    let original_text = original_src.text;
    let start_line = bullet.line()?;
    let line_count = bullet.lines().len();
    let abs_start = nth_line_start(original_text, start_line)?;
    let abs_end =
        nth_line_start(original_text, start_line + line_count).unwrap_or(original_text.len());
    Some(rowan::TextRange::new(
        (abs_start as u32).into(),
        (abs_end as u32).into(),
    ))
}

/// Return the byte offset of the start of the n-th 0-indexed line in
/// `text`. `nth_line_start(text, 0)` is `Some(0)`. Returns `None` if the
/// text has fewer than `n` newlines.
fn nth_line_start(text: &str, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(0);
    }
    let mut count = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == n {
                return Some(i + 1);
            }
        }
    }
    None
}

fn render_bullet_block(new_lines: &[String]) -> String {
    let mut out = String::new();
    for line in new_lines {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn dep3_action_to_text_edits(
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

fn dep3_action_range(
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

fn watch_action_to_text_edits(
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

fn watch_action_range(
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

fn yaml_action_to_text_edits(
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

fn yaml_action_range(
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

fn deb822_action_to_text_edits(
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

/// Find the paragraph matching `selector` in `control`. Returns the
/// underlying deb822 `Paragraph` (read-only) so callers can probe its
/// entries' ranges.
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
fn copyright_action_to_text_edits(
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
            // No detector emits this against debian/copyright in
            // practice, and the copyright wrappers don't have a typed
            // reorder API — leave it unimplemented for now rather than
            // silently dropping it.
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

fn find_paragraph(
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
fn find_paragraph_in_deb822(
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

fn find_entry_in_paragraph(
    paragraph: &deb822_lossless::Paragraph,
    field: &str,
) -> Option<deb822_lossless::Entry> {
    paragraph
        .entries()
        .find(|e| e.key().as_deref() == Some(field))
}

fn set_field_edits(
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
    }
 else {
        // Field is missing — insert at the end of the paragraph. The
        // paragraph's text_range() ends just after its last entry's
        // trailing newline, so we insert there. (A future improvement
        // would consult canonical field ordering from
        // debian-control's SOURCE_FIELD_ORDER.)
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
fn absorb_trailing_blank_line(text: &str, end: usize) -> usize {
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

fn append_paragraph_edits(
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
    let by_key: std::collections::HashMap<&str, &str> = participants
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

fn filesystem_action_to_text_edits(
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
                range: full_document_range(original_text),
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
                range: full_document_range(original_text),
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

fn action_file(action: &Action) -> Option<&Path> {
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
        // These action kinds aren't yet wired into the LSP translator
        // (see `is_action_translatable` for the full list and rationale).
        // Returning `None` here matches the dispatch in
        // `locate_action_target` / `action_to_text_edits`.
        Action::Systemd(_)
        | Action::DesktopIni(_)
        | Action::Makefile(_)
        | Action::LintianOverrides(_)
        | Action::Maintscript(_)
        | Action::Debcargo(_)
        | Action::RunCommand(_) => return None,
    })
}

/// Compute a `Range` covering the entire document, in LSP coordinates
/// (line / utf-16 character).
fn full_document_range(text: &str) -> Range {
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
