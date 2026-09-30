use crate::debhelper::parser::ParsedLine;
use crate::position::utf16_len;
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
///
/// `text` must be the buffer the parse was taken from. `normalize` is applied
/// to both the source and destination halves of an entry when computing the
/// collision key.
pub fn find_duplicate_entries(
    text: &str,
    parsed: &[ParsedLine],
    shape: LineShape,
    normalize: impl Fn(&str) -> String,
) -> Vec<DiagnosticIssue> {
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (line_num, parsed_line) in parsed.iter().enumerate() {
        if parsed_line.line.comment.is_some() || parsed_line.line.words.is_empty() {
            continue;
        }
        let line = &text[parsed_line.range.clone()];
        let words: Vec<&str> = parsed_line
            .line
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
                    range: line_range(line, line_num),
                });
            }
        }
    }

    issues
}

/// The entries a line declares.
fn entries<'a>(
    words: &'a [&'a str],
    shape: LineShape,
) -> impl Iterator<Item = (&'a str, Option<&'a str>)> + 'a {
    let split = match shape {
        LineShape::WordsWithDestination => words.split_last().filter(|(_, s)| !s.is_empty()),
        LineShape::Words => None,
    };
    let (sources, destination) = match split {
        Some((dest, sources)) => (sources, Some(*dest)),
        None => (words, None),
    };
    sources.iter().map(move |&s| (s, destination))
}

/// Build the LSP range spanning an entire line.
fn line_range(line: &str, line_num: usize) -> Range {
    let start = Position::new(line_num as u32, 0);
    let end = Position::new(line_num as u32, utf16_len(line));
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
    text: &str,
    parsed: &[ParsedLine],
    shape: LineShape,
    normalize: impl Fn(&str) -> String,
) -> Vec<Diagnostic> {
    find_duplicate_entries(text, parsed, shape, normalize)
        .into_iter()
        .map(issue_to_diagnostic)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debhelper::parser::parse_buffer;

    fn issues_with(text: &str, shape: LineShape) -> Vec<DiagnosticIssue> {
        let parsed = parse_buffer(text);
        find_duplicate_entries(text, &parsed, shape, |e| e.to_string())
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
        let parsed = parse_buffer(text);
        let diags = find_duplicate_entries(text, &parsed, LineShape::Words, |e| e.to_lowercase());
        assert_eq!(diags.len(), 1);
    }
}
