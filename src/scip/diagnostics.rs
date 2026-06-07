//! Attach diagnostics to SCIP documents.
//!
//! Diagnostics are produced elsewhere (the LSP diagnostic pipeline) and handed
//! to this module as plain data so the scip layer stays free of LSP types. Each
//! diagnostic is carried on a symbol-less [`Occurrence`] whose `diagnostics`
//! field is populated, which is how SCIP models range-scoped diagnostics.

use crate::scip::linetable::LineTable;
use scip::types::{Diagnostic, Occurrence, Severity};

/// Severity of a diagnostic, mirroring the SCIP severity levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl DiagnosticSeverity {
    fn to_scip(self) -> Severity {
        match self {
            DiagnosticSeverity::Error => Severity::Error,
            DiagnosticSeverity::Warning => Severity::Warning,
            DiagnosticSeverity::Information => Severity::Information,
            DiagnosticSeverity::Hint => Severity::Hint,
        }
    }
}

/// A diagnostic to attach to a document, addressed by byte range.
#[derive(Clone, Debug)]
pub struct ScipDiagnostic {
    /// Half-open byte range `[start, end)` within the document text.
    pub range: (u32, u32),
    /// Severity of the diagnostic.
    pub severity: DiagnosticSeverity,
    /// Optional diagnostic code (e.g. a lintian tag name).
    pub code: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Producer of the diagnostic (e.g. `lintian-brush`).
    pub source: String,
}

/// Attach `diags` to the document at `relative_path` within `index`.
///
/// `text` is the document's source, used to convert byte offsets to SCIP
/// line/column ranges. Each diagnostic becomes a symbol-less [`Occurrence`]
/// carrying a single [`Diagnostic`]. Diagnostics whose target document is not
/// present in the index are dropped.
pub fn attach(
    index: &mut scip::types::Index,
    relative_path: &str,
    text: &str,
    diags: &[ScipDiagnostic],
) {
    if diags.is_empty() {
        return;
    }
    let Some(doc) = index
        .documents
        .iter_mut()
        .find(|d| d.relative_path == relative_path)
    else {
        return;
    };
    let lines = LineTable::new(text);
    for d in diags {
        doc.occurrences.push(diagnostic_occurrence(&lines, d));
    }
}

fn diagnostic_occurrence(lines: &LineTable, d: &ScipDiagnostic) -> Occurrence {
    Occurrence {
        range: lines.range(d.range.0, d.range.1),
        diagnostics: vec![Diagnostic {
            severity: d.severity.to_scip().into(),
            code: d.code.clone().unwrap_or_default(),
            message: d.message.clone(),
            source: d.source.clone(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::{Document, Index};

    fn doc(path: &str) -> Document {
        Document {
            relative_path: path.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn attaches_to_matching_document() {
        let mut index = Index {
            documents: vec![doc("debian/control")],
            ..Default::default()
        };
        let text = "Source: hello\n";
        attach(
            &mut index,
            "debian/control",
            text,
            &[ScipDiagnostic {
                range: (8, 13),
                severity: DiagnosticSeverity::Warning,
                code: Some("some-tag".to_owned()),
                message: "watch out".to_owned(),
                source: "lintian-brush".to_owned(),
            }],
        );
        let occ = &index.documents[0].occurrences;
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].range, vec![0, 8, 0, 13]);
        assert!(occ[0].symbol.is_empty());
        assert_eq!(occ[0].diagnostics.len(), 1);
        assert_eq!(occ[0].diagnostics[0].message, "watch out");
        assert_eq!(occ[0].diagnostics[0].code, "some-tag");
        assert_eq!(
            occ[0].diagnostics[0].severity.value(),
            Severity::Warning as i32
        );
    }

    #[test]
    fn drops_diagnostics_for_missing_document() {
        let mut index = Index {
            documents: vec![doc("debian/control")],
            ..Default::default()
        };
        attach(
            &mut index,
            "debian/rules",
            "x\n",
            &[ScipDiagnostic {
                range: (0, 1),
                severity: DiagnosticSeverity::Error,
                code: None,
                message: "nope".to_owned(),
                source: "builtin".to_owned(),
            }],
        );
        assert!(index.documents[0].occurrences.is_empty());
    }
}
