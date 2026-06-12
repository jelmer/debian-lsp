use crate::position::Source;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range, Uri};

use super::fields::CONFFILES_FLAGS;

/// All types of diagnostic issues in a debian/conffiles file.
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    /// Line is empty or whitespace only
    EmptyLine { range: Range },
    /// Path is not absolute
    RelativePath { path: String, range: Range },
    /// Unknown flag (not remove-on-upgrade)
    UnknownFlag { flag: String, range: Range },
    /// remove-on-upgrade file exists in the package staging directory
    RemoveOnUpgradeFileExists { path: String, range: Range },
    /// File without flag not found in staging directory
    FileNotInStaging { path: String, range: Range },
    /// Duplicate entry
    DuplicateEntry { path: String, range: Range },
    /// Too many tokens
    TooManyTokens { range: Range },
}

/// Find all diagnostic issues in a debian/conffiles file.
pub fn find_all_issues(src: Source<'_>, uri: &Uri) -> Vec<DiagnosticIssue> {
    let flag = CONFFILES_FLAGS[0].0;
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let debian_dir = uri
        .to_file_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    for (line_num, line) in src.text.lines().enumerate() {
        let trimmed = line.trim();
        let line_range = line_range(src, line_num);

        // Empty or whitespace-only line
        if trimmed.is_empty() {
            issues.push(DiagnosticIssue::EmptyLine { range: line_range });
            continue;
        }

        // Skip comments
        if trimmed.starts_with('#') {
            continue;
        }

        // Too many tokens
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() > 2 || (tokens.len() == 2 && tokens[0] != flag) {
            issues.push(DiagnosticIssue::TooManyTokens { range: line_range });
            continue;
        }

        // Parse flag and path
        let (has_flag, path) = if let Some(rest) = trimmed.strip_prefix(flag) {
            (true, rest.trim())
        } else if trimmed.starts_with('/') {
            (false, trimmed)
        } else {
            // Unknown flag or relative path
            let first_token = trimmed.split_whitespace().next().unwrap_or(trimmed);
            if first_token.contains('/') || !first_token.contains('-') {
                issues.push(DiagnosticIssue::RelativePath {
                    path: first_token.to_string(),
                    range: line_range,
                });
            } else {
                issues.push(DiagnosticIssue::UnknownFlag {
                    flag: first_token.to_string(),
                    range: line_range,
                });
            }
            continue;
        };

        // Path must be absolute
        if !path.starts_with('/') {
            issues.push(DiagnosticIssue::RelativePath {
                path: path.to_string(),
                range: line_range,
            });
            continue;
        }

        // Duplicate check
        let key = format!("{}{}", if has_flag { "rou:" } else { "" }, path);
        if !seen.insert(key) {
            issues.push(DiagnosticIssue::DuplicateEntry {
                path: path.to_string(),
                range: line_range,
            });
            continue;
        }

        // Check staging directory
        if let Some(ref debian_dir) = debian_dir {
            let rel = path.trim_start_matches('/');
            let exists_in_staging = std::fs::read_dir(debian_dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.path().join(rel).exists());

            if has_flag && exists_in_staging {
                issues.push(DiagnosticIssue::RemoveOnUpgradeFileExists {
                    path: path.to_string(),
                    range: line_range,
                });
            } else if !has_flag && !exists_in_staging {
                issues.push(DiagnosticIssue::FileNotInStaging {
                    path: path.to_string(),
                    range: line_range,
                });
            }
        }
    }

    issues
}

/// Build the LSP Range for an entire line.
fn line_range(src: Source<'_>, line_num: usize) -> Range {
    let line = src.text.lines().nth(line_num).unwrap_or("");
    let start = tower_lsp_server::ls_types::Position::new(line_num as u32, 0);
    let end = tower_lsp_server::ls_types::Position::new(
        line_num as u32,
        crate::position::utf16_len(line),
    );
    Range::new(start, end)
}

/// Convert a DiagnosticIssue to an LSP Diagnostic.
pub fn issue_to_diagnostic(issue: DiagnosticIssue) -> Diagnostic {
    match issue {
        DiagnosticIssue::EmptyLine { range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("empty-line".to_string())),
            source: Some("debian-lsp".to_string()),
            message: "Empty lines are not allowed in conffiles".to_string(),
            ..Default::default()
        },
        DiagnosticIssue::RelativePath { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("relative-path".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("'{}' is not an absolute path", path),
            ..Default::default()
        },
        DiagnosticIssue::UnknownFlag { flag, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("unknown-flag".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("Unknown flag '{}', only 'remove-on-upgrade' is valid", flag),
            ..Default::default()
        },
        DiagnosticIssue::RemoveOnUpgradeFileExists { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("remove-on-upgrade-exists".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!(
                "'{}' is marked remove-on-upgrade but exists in the package - dpkg-deb will refuse to build",
                path
            ),
            ..Default::default()
        },
        DiagnosticIssue::FileNotInStaging { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("file-not-in-staging".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!(
                "'{}' not found in debhelper staging directory - dpkg will silently ignore it",
                path
            ),
            ..Default::default()
        },
        DiagnosticIssue::DuplicateEntry { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("duplicate-entry".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("Duplicate entry '{}'", path),
            ..Default::default()
        },
        DiagnosticIssue::TooManyTokens { range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("too-many-tokens".to_string())),
            source: Some("debian-lsp".to_string()),
            message: "A conffiles entry must be a single absolute path, optionally preceded by 'remove-on-upgrade'".to_string(),
            ..Default::default()
        },
    }
}

/// Get all LSP diagnostics for a debian/conffiles file.
pub fn get_diagnostics(src: Source<'_>, uri: &Uri) -> Vec<Diagnostic> {
    find_all_issues(src, uri)
        .into_iter()
        .map(issue_to_diagnostic)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn make_uri() -> Uri {
        "file:///tmp/debian/conffiles".parse().unwrap()
    }

    fn issues(text: &str) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_all_issues(src, &make_uri())
    }

    #[test]
    fn test_empty_line_is_error() {
        let diags = issues("\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::EmptyLine { .. })));
    }

    #[test]
    fn test_relative_path_is_error() {
        let diags = issues("etc/myapp/config.conf\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::RelativePath { .. })));
    }

    #[test]
    fn test_unknown_flag_is_error() {
        let diags = issues("bad-flag\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::UnknownFlag { .. })));
    }

    #[test]
    fn test_duplicate_entry_is_error() {
        let diags = issues("/etc/foo\n/etc/foo\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::DuplicateEntry { .. })));
    }

    #[test]
    fn test_valid_path_no_issues() {
        let diags = issues("/etc/myapp/config.conf\n");
        assert!(!diags.iter().any(|d| matches!(
            d,
            DiagnosticIssue::EmptyLine { .. }
                | DiagnosticIssue::RelativePath { .. }
                | DiagnosticIssue::UnknownFlag { .. }
                | DiagnosticIssue::DuplicateEntry { .. }
        )));
    }

    #[test]
    fn test_comment_no_issues() {
        let diags = issues("# this is a comment\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_too_many_tokens_is_error() {
        let diags = issues("/etc/myapp/config.conf /etc/myapp/extra.conf\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::TooManyTokens { .. })));
    }

    #[test]
    fn test_remove_on_upgrade_with_path_is_valid() {
        let diags = issues("remove-on-upgrade /etc/myapp/old.conf\n");
        assert!(!diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::TooManyTokens { .. })));
    }
}
