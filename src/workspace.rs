use text_size::TextRange;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range, Url};

/// Information about a field casing issue
#[derive(Debug, Clone)]
pub struct FieldCasingIssue {
    pub field_name: String,
    pub standard_name: String,
    pub field_range: TextRange,
}

/// All types of diagnostic issues that can be found in a control file
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    FieldCasing(FieldCasingIssue),
    ParseError { message: String, range: Option<TextRange> },
}

#[salsa::input]
pub struct SourceFile {
    pub url: Url,
    pub text: String,
}

// Store the Parse type directly - it's thread-safe now!
#[salsa::tracked]
pub fn parse_control(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> debian_control::lossless::Parse<debian_control::lossless::Control> {
    let text = file.text(db);
    debian_control::lossless::Control::parse(&text)
}

// The actual database implementation
#[salsa::db]
#[derive(Clone, Default)]
pub struct Workspace {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for Workspace {}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_file(&mut self, url: Url, text: String) -> SourceFile {
        SourceFile::new(self, url, text)
    }

    pub fn get_parsed_control(
        &self,
        file: SourceFile,
    ) -> debian_control::lossless::Parse<debian_control::lossless::Control> {
        parse_control(self, file)
    }

    pub fn source_text(&self, file: SourceFile) -> String {
        file.text(self).clone()
    }

    /// Find all field casing issues in the document, optionally within a specific range
    pub fn find_field_casing_issues(
        &self,
        file: SourceFile,
        range: Option<TextRange>,
    ) -> Vec<FieldCasingIssue> {
        let mut issues = Vec::new();
        let parsed = self.get_parsed_control(file);

        if let Ok(control) = parsed.to_result() {
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    let entry_range = entry.text_range();

                    // If a range is specified, check if this entry is within it
                    if let Some(filter_range) = range {
                        if entry_range.start() >= filter_range.end()
                            || entry_range.end() <= filter_range.start()
                        {
                            continue; // Skip entries outside the range
                        }
                    }

                    if let Some(field_name) = entry.key() {
                        if let Some(standard_name) =
                            crate::control::get_standard_field_name(&field_name)
                        {
                            if field_name != standard_name {
                                let field_range = TextRange::new(
                                    entry_range.start(),
                                    entry_range.start()
                                        + text_size::TextSize::of(field_name.as_str()),
                                );

                                issues.push(FieldCasingIssue {
                                    field_name,
                                    standard_name: standard_name.to_string(),
                                    field_range,
                                });
                            }
                        }
                    }
                }
            }
        }

        issues
    }

    pub fn get_diagnostics(&self, file: SourceFile) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let parsed = self.get_parsed_control(file);
        let source_text = self.source_text(file);

        // Report parse errors
        for error in parsed.errors() {
            diagnostics.push(Diagnostic {
                range: Range {
                    start: tower_lsp::lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                    end: tower_lsp::lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: error.clone(),
                ..Default::default()
            });
        }

        // Check for field casing issues using centralized logic
        for issue in self.find_field_casing_issues(file, None) {
            let lsp_range =
                crate::position::text_range_to_lsp_range(&source_text, issue.field_range);

            diagnostics.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "field-casing".to_string(),
                )),
                source: Some("debian-lsp".to_string()),
                message: format!(
                    "Field name '{}' should be '{}'",
                    issue.field_name, issue.standard_name
                ),
                ..Default::default()
            });
        }

        diagnostics
    }

    pub fn get_diagnostics_in_range(&self, file: SourceFile, range: TextRange) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let source_text = self.source_text(file);

        // Check for field casing issues only in the given range using centralized logic
        for issue in self.find_field_casing_issues(file, Some(range)) {
            let lsp_range =
                crate::position::text_range_to_lsp_range(&source_text, issue.field_range);

            diagnostics.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "field-casing".to_string(),
                )),
                source: Some("debian-lsp".to_string()),
                message: format!(
                    "Field name '{}' should be '{}'",
                    issue.field_name, issue.standard_name
                ),
                ..Default::default()
            });
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_parse_control_with_correct_casing() {
        let mut workspace = Workspace::new();
        let url = Url::parse("file:///debian/control").unwrap();
        let content = "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: amd64\nDescription: A test package\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        assert!(parsed.errors().is_empty());

        if let Ok(control) = parsed.to_result() {
            let field_count: usize = control
                .as_deb822()
                .paragraphs()
                .map(|p| p.entries().count())
                .sum();
            assert_eq!(field_count, 5);
        } else {
            panic!("Failed to parse valid control file");
        }
    }

    #[test]
    fn test_parse_control_with_incorrect_casing() {
        let mut workspace = Workspace::new();
        let url = Url::parse("file:///debian/control").unwrap();
        let content = "source: test-package\nmaintainer: Test <test@example.com>\n\npackage: test-package\narchitecture: amd64\ndescription: A test package\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        assert!(parsed.errors().is_empty());

        if let Ok(control) = parsed.to_result() {
            // Check that incorrect casing is preserved
            let mut field_names = Vec::new();
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    if let Some(name) = entry.key() {
                        field_names.push(name);
                    }
                }
            }
            assert!(field_names.contains(&"source".to_string()));
            assert!(field_names.contains(&"maintainer".to_string()));
        }
    }

    #[test]
    fn test_parse_control_with_invalid_content() {
        let mut workspace = Workspace::new();
        let url = Url::parse("file:///debian/control").unwrap();
        let content = "invalid debian control content without proper format";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        // The parser is quite permissive, so check if we get a valid result
        if let Ok(control) = parsed.to_result() {
            // Even invalid content might parse to some degree
            let field_count: usize = control
                .as_deb822()
                .paragraphs()
                .map(|p| p.entries().count())
                .sum();
            // But it should have minimal fields
            assert!(field_count <= 1);
        }
    }
}
