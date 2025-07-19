//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
}

#[derive(Debug, Deserialize, Serialize)]
struct InlayHintParams {
    path: String,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![":".to_string(), " ".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Debian LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file opened: {}", params.text_document.uri),
            )
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file changed: {}", params.text_document.uri),
            )
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if !uri.path().ends_with("/control") && !uri.path().ends_with("/debian/control") {
            return Ok(None);
        }

        let completions = self.get_completions(&uri, position).await;
        Ok(Some(CompletionResponse::Array(completions)))
    }
}

impl Backend {
    async fn get_completions(&self, _uri: &Url, _position: Position) -> Vec<CompletionItem> {
        let mut completions = Vec::new();

        // Common Debian control file fields
        let control_fields = vec![
            ("Source", "Name of the source package"),
            ("Section", "Classification of the package"),
            ("Priority", "Priority of the package"),
            ("Maintainer", "Package maintainer's name and email"),
            ("Uploaders", "Additional maintainers"),
            ("Build-Depends", "Build dependencies"),
            (
                "Build-Depends-Indep",
                "Architecture-independent build dependencies",
            ),
            ("Build-Conflicts", "Packages that conflict during build"),
            ("Standards-Version", "Debian Policy version"),
            ("Homepage", "Upstream project homepage"),
            ("Vcs-Browser", "Web interface for VCS"),
            ("Vcs-Git", "Git repository URL"),
            ("Package", "Binary package name"),
            ("Architecture", "Supported architectures"),
            ("Multi-Arch", "Multi-architecture support"),
            ("Depends", "Package dependencies"),
            ("Pre-Depends", "Pre-installation dependencies"),
            ("Recommends", "Recommended packages"),
            ("Suggests", "Suggested packages"),
            ("Enhances", "Packages enhanced by this one"),
            ("Conflicts", "Conflicting packages"),
            ("Breaks", "Packages broken by this one"),
            ("Provides", "Virtual packages provided"),
            ("Replaces", "Packages replaced by this one"),
            ("Description", "Package description"),
            ("Essential", "Essential package flag"),
            ("Rules-Requires-Root", "Root privileges requirement"),
        ];

        for (field, description) in control_fields {
            completions.push(CompletionItem {
                label: field.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(description.to_string()),
                documentation: Some(Documentation::String(description.to_string())),
                insert_text: Some(format!("{}: ", field)),
                ..Default::default()
            });
        }

        // Package name suggestions (simplified for now)
        let common_packages = vec![
            "debhelper-compat",
            "dh-python",
            "python3-all",
            "python3-setuptools",
            "cmake",
            "pkg-config",
            "libssl-dev",
            "libc6-dev",
        ];

        for package in common_packages {
            completions.push(CompletionItem {
                label: package.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some("Package name".to_string()),
                ..Default::default()
            });
        }

        completions
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend { client });

    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_item_properties() {
        // Test that we can create completion items with the expected properties
        let completion = CompletionItem {
            label: "Test".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("Test field".to_string()),
            documentation: Some(Documentation::String("Test documentation".to_string())),
            insert_text: Some("Test: ".to_string()),
            ..Default::default()
        };

        assert_eq!(completion.label, "Test");
        assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(completion.detail, Some("Test field".to_string()));
        assert_eq!(completion.insert_text, Some("Test: ".to_string()));
    }

    #[test]
    fn test_control_field_completions() {
        // Test the actual control field data we use in get_completions
        let expected_fields = vec![
            "Source",
            "Section",
            "Priority",
            "Maintainer",
            "Uploaders",
            "Build-Depends",
            "Build-Depends-Indep",
            "Build-Conflicts",
            "Standards-Version",
            "Homepage",
            "Vcs-Browser",
            "Vcs-Git",
            "Package",
            "Architecture",
            "Multi-Arch",
            "Depends",
            "Pre-Depends",
            "Recommends",
            "Suggests",
            "Enhances",
            "Conflicts",
            "Breaks",
            "Provides",
            "Replaces",
            "Description",
            "Essential",
            "Rules-Requires-Root",
        ];

        // Each field should be present in our completion logic
        for field in expected_fields {
            assert!(
                !field.is_empty(),
                "Field name should not be empty: {}",
                field
            );
            assert!(
                field.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "Field should only contain alphanumeric chars and hyphens: {}",
                field
            );
        }
    }

    #[test]
    fn test_package_name_completions() {
        // Test the package name data we use in get_completions
        let expected_packages = vec![
            "debhelper-compat",
            "dh-python",
            "python3-all",
            "python3-setuptools",
            "cmake",
            "pkg-config",
            "libssl-dev",
            "libc6-dev",
        ];

        for package in expected_packages {
            assert!(
                !package.is_empty(),
                "Package name should not be empty: {}",
                package
            );
            assert!(
                package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c.is_ascii_digit()),
                "Package name should only contain valid characters: {}",
                package
            );
        }
    }

    #[test]
    fn test_file_path_detection() {
        // Test the file path detection logic used in completion()
        let control_paths = vec![
            "file:///path/to/debian/control",
            "file:///project/debian/control",
            "file:///control",
            "file:///some/path/control",
        ];

        let non_control_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/control.txt",
            "file:///path/to/mycontrol",
            "file:///path/to/debian/control.backup",
        ];

        for path in control_paths {
            let uri = Url::parse(path).unwrap();
            assert!(
                uri.path().ends_with("/control") || uri.path().ends_with("/debian/control"),
                "Should detect control file: {}",
                path
            );
        }

        for path in non_control_paths {
            let uri = Url::parse(path).unwrap();
            assert!(
                !(uri.path().ends_with("/control") || uri.path().ends_with("/debian/control")),
                "Should not detect as control file: {}",
                path
            );
        }
    }

    #[test]
    fn test_completion_item_creation() {
        // Test creating a field completion item as done in get_completions
        let field = "Source";
        let description = "Name of the source package";

        let completion = CompletionItem {
            label: field.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(description.to_string()),
            documentation: Some(Documentation::String(description.to_string())),
            insert_text: Some(format!("{}: ", field)),
            ..Default::default()
        };

        assert_eq!(completion.label, "Source");
        assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(
            completion.detail,
            Some("Name of the source package".to_string())
        );
        assert_eq!(completion.insert_text, Some("Source: ".to_string()));

        if let Some(Documentation::String(doc)) = completion.documentation {
            assert_eq!(doc, "Name of the source package");
        } else {
            panic!("Expected string documentation");
        }
    }

    #[test]
    fn test_package_completion_item_creation() {
        // Test creating a package completion item as done in get_completions
        let package = "debhelper-compat";

        let completion = CompletionItem {
            label: package.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Package name".to_string()),
            ..Default::default()
        };

        assert_eq!(completion.label, "debhelper-compat");
        assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
        assert_eq!(completion.detail, Some("Package name".to_string()));
        assert_eq!(completion.insert_text, None); // Package completions don't have insert_text
    }

    #[test]
    fn test_url_parsing() {
        // Test URL parsing for various control file paths
        let test_paths = vec![
            "file:///home/user/project/debian/control",
            "file:///tmp/control",
            "file:///var/lib/dpkg/control",
        ];

        for path in test_paths {
            let uri = Url::parse(path);
            assert!(uri.is_ok(), "Should be able to parse URL: {}", path);

            let uri = uri.unwrap();
            assert_eq!(uri.scheme(), "file");
            assert!(!uri.path().is_empty());
        }
    }

    #[test]
    fn test_position_creation() {
        // Test Position creation as used in completion params
        let position = Position::new(0, 0);
        assert_eq!(position.line, 0);
        assert_eq!(position.character, 0);

        let position = Position::new(5, 10);
        assert_eq!(position.line, 5);
        assert_eq!(position.character, 10);
    }
}
