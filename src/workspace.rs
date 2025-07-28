use tower_lsp::lsp_types::Url;

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
