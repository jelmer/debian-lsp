//! Native spell-checking for prose in Debian packaging files.
//!
//! Uses the `typos` engine plus the `typos-dict` correction list (the same
//! data behind `typos-cli`), so there is no Python runtime dependency and the
//! check runs inline in the LSP. The engine is correction-based: it only
//! flags words it has a known fix for, which keeps false positives low on the
//! package names, acronyms and version strings that fill packaging files.
//!
//! This module is file-format agnostic: it checks plain strings and builds
//! the LSP diagnostics from findings that callers have already mapped to
//! source ranges. The per-format wiring (which fields hold prose, how to map
//! offsets back to the source) lives next to each file type, e.g.
//! [`crate::control::spelling`].

mod dict;

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

/// A single spelling finding within a checked string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellingFinding {
    /// Byte offset of the typo within the checked buffer.
    pub byte_offset: usize,
    /// The misspelled word as it appears in the buffer.
    pub typo: String,
    /// Suggested corrections, best-first. May be empty when the word is
    /// known-wrong but has no single agreed correction.
    pub corrections: Vec<String>,
}

impl SpellingFinding {
    /// Byte range of the typo within the checked buffer.
    pub fn span(&self) -> std::ops::Range<usize> {
        self.byte_offset..self.byte_offset + self.typo.len()
    }
}

/// Check a single string for spelling mistakes.
///
/// Offsets in the returned findings are relative to `text`.
pub fn check_text(text: &str) -> Vec<SpellingFinding> {
    let tokenizer = typos::tokens::Tokenizer::new();
    let dictionary = dict::WordListDictionary;

    typos::check_str(text, &tokenizer, &dictionary)
        .map(|typo| SpellingFinding {
            byte_offset: typo.byte_offset,
            typo: typo.typo.into_owned(),
            corrections: match typo.corrections {
                typos::Status::Corrections(c) => c.into_iter().map(|c| c.into_owned()).collect(),
                // `Invalid` means known-wrong with no correction; `Valid`
                // never reaches here (the iterator only yields typos).
                _ => Vec::new(),
            },
        })
        .collect()
}

/// A spelling finding located at an LSP range within a source file.
pub struct LocatedFinding {
    pub range: Range,
    pub finding: SpellingFinding,
}

/// Build an LSP diagnostic for a spelling finding at the given range.
pub fn make_diagnostic(range: Range, finding: &SpellingFinding) -> Diagnostic {
    let message = if finding.corrections.is_empty() {
        format!("\"{}\" is a common misspelling", finding.typo)
    } else {
        format!(
            "\"{}\" should be \"{}\"",
            finding.typo,
            finding.corrections.join("\" or \"")
        )
    };

    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String("spelling".to_string())),
        source: Some("debian-lsp".to_string()),
        message,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_text_finds_typo() {
        let findings = check_text("This packge is teh best");
        let typos: Vec<&str> = findings.iter().map(|f| f.typo.as_str()).collect();
        assert_eq!(typos, vec!["packge", "teh"]);
    }

    #[test]
    fn test_check_text_offers_correction() {
        let findings = check_text("recieve");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].typo, "recieve");
        assert_eq!(findings[0].corrections, vec!["receive".to_string()]);
    }

    #[test]
    fn test_check_text_preserves_case() {
        let findings = check_text("Recieve");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].corrections, vec!["Receive".to_string()]);
    }

    #[test]
    fn test_check_text_clean() {
        assert_eq!(check_text("This package is the best"), vec![]);
    }

    #[test]
    fn test_check_text_byte_offset() {
        let findings = check_text("a recieve b");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].byte_offset, 2);
        assert_eq!(findings[0].span(), 2..9);
    }
}
