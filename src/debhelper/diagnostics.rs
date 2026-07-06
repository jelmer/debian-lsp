use crate::debhelper::parser::parse_line;
use crate::position::Source;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

/// How a file's lines map to the entries it declares.
#[derive(Debug, Clone, Copy)]
pub enum LineShape {
    /// Every word on a line stands on its own, as in dirs or manpages.
    Words,
    /// The last word of a multi-word line is the destination, as in install.
    WordsWithDestination,
}

/// A diagnostic issue in a line-oriented debhelper file.
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    /// An entry that repeats one already listed above it.
    DuplicateEntry {
        /// The entry text, the destination appended when there is one.
        path: String,
        /// Range of the offending line.
        range: Range,
    },
}

/// Find entries that repeat an earlier one.
pub fn find_duplicate_entries(
    src: Source<'_>,
    shape: LineShape,
    normalize: impl Fn(&str) -> String,
) -> Vec<DiagnosticIssue> {
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (line_num, line) in src.text.lines().enumerate() {
        // Take the words straight from the parser so the notion of a comment,
        // a blank line, and where the tokens are lives in one place.
        let parsed = parse_line(line);
        if parsed.comment.is_some() || parsed.words.is_empty() {
            continue;
        }
        let words: Vec<&str> = parsed
            .words
            .iter()
            .map(|word| &line[word.range.clone()])
            .collect();

        for (path, destination) in entries(&words, shape) {
            let key = (normalize(path), destination.map(&normalize));
            if !seen.insert(key) {
                issues.push(DiagnosticIssue::DuplicateEntry {
                    path: match destination {
                        Some(destination) => format!("{path} {destination}"),
                        None => path.to_string(),
                    },
                    range: line_range(src, line_num),
                });
            }
        }
    }

    issues
}

/// The entries a line declares.
fn entries<'a>(words: &[&'a str], shape: LineShape) -> Vec<(&'a str, Option<&'a str>)> {
    match shape {
        LineShape::WordsWithDestination if words.len() > 1 => {
            let (destination, sources) = words.split_last().unwrap();
            sources.iter().map(|&s| (s, Some(*destination))).collect()
        }
        _ => words.iter().map(|&word| (word, None)).collect(),
    }
}

/// Build the LSP range spanning an entire line.
fn line_range(src: Source<'_>, line_num: usize) -> Range {
    let line = src.text.lines().nth(line_num).unwrap_or("");
    let start = Position::new(line_num as u32, 0);
    let end = Position::new(line_num as u32, crate::position::utf16_len(line));
    Range::new(start, end)
}

/// Turn an issue into an LSP diagnostic.
pub fn issue_to_diagnostic(issue: DiagnosticIssue) -> Diagnostic {
    match issue {
        DiagnosticIssue::DuplicateEntry { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("duplicate-entry".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("Duplicate entry '{}'", path),
            ..Default::default()
        },
    }
}

/// All LSP diagnostics for a line-oriented debhelper file, keyed by `normalize`.
pub fn get_diagnostics(
    src: Source<'_>,
    shape: LineShape,
    normalize: impl Fn(&str) -> String,
) -> Vec<Diagnostic> {
    find_duplicate_entries(src, shape, normalize)
        .into_iter()
        .map(issue_to_diagnostic)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn issues_with(text: &str, shape: LineShape) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_duplicate_entries(src, shape, |e| e.to_string())
    }

    fn issues(text: &str) -> Vec<DiagnosticIssue> {
        issues_with(text, LineShape::Words)
    }

    #[test]
    fn repeated_entry_is_flagged() {
        let diags = issues("usr/share/myapp\nusr/share/myapp\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::DuplicateEntry { .. })));
    }

    #[test]
    fn distinct_entries_are_clean() {
        assert!(issues("usr/share/myapp\nusr/lib/myapp\n").is_empty());
    }

    #[test]
    fn blank_lines_and_comments_are_ignored() {
        assert!(issues("\n# a comment\nusr/share/myapp\n").is_empty());
    }

    #[test]
    fn a_word_repeated_on_one_line_is_flagged() {
        let diags = issues("usr/bin usr/bin\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn internal_whitespace_does_not_split_an_entry() {
        let diags = issues_with(
            "foo   usr/bin\nfoo usr/bin\n",
            LineShape::WordsWithDestination,
        );
        assert_eq!(diags.len(), 1);
        let DiagnosticIssue::DuplicateEntry { path, .. } = &diags[0];
        assert_eq!(path, "foo usr/bin");
    }

    #[test]
    fn a_source_installed_twice_into_the_same_place_is_flagged() {
        let diags = issues_with(
            "foo bar usr/bin\nfoo usr/bin\n",
            LineShape::WordsWithDestination,
        );
        assert_eq!(diags.len(), 1);
        let DiagnosticIssue::DuplicateEntry { path, .. } = &diags[0];
        assert_eq!(path, "foo usr/bin");
    }

    #[test]
    fn the_same_source_in_another_destination_is_clean() {
        assert!(issues_with(
            "foo usr/bin\nfoo usr/lib\n",
            LineShape::WordsWithDestination
        )
        .is_empty());
    }

    #[test]
    fn a_lone_source_has_no_destination() {
        assert!(issues_with("foo\nfoo usr/bin\n", LineShape::WordsWithDestination).is_empty());
    }

    #[test]
    fn normalize_key_controls_what_collides() {
        let text = "Foo\nfoo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let diags = find_duplicate_entries(src, LineShape::Words, |e| e.to_lowercase());
        assert_eq!(diags.len(), 1);
    }
}
