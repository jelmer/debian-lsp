//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod control;
mod position;
mod workspace;

use position::text_range_to_lsp_range;
use std::collections::HashMap;
use text_size::{TextRange, TextSize};
use workspace::Workspace;

struct Backend {
    client: Client,
    workspace: Arc<Mutex<Workspace>>,
    files: Arc<Mutex<HashMap<Url, workspace::SourceFile>>>,
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
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
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

        if control::is_control_file(&params.text_document.uri) {
            let mut workspace = self.workspace.lock().await;
            let file = workspace.update_file(
                params.text_document.uri.clone(),
                params.text_document.text.clone(),
            );
            let mut files = self.files.lock().await;
            files.insert(params.text_document.uri.clone(), file);
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file changed: {}", params.text_document.uri),
            )
            .await;

        if control::is_control_file(&params.text_document.uri) {
            let mut workspace = self.workspace.lock().await;
            let mut files = self.files.lock().await;

            // Apply the content changes
            if let Some(changes) = params.content_changes.first() {
                // Update or create the file
                let file =
                    workspace.update_file(params.text_document.uri.clone(), changes.text.clone());
                files.insert(params.text_document.uri.clone(), file);
            }
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let completions = control::get_completions(&uri, position);

        if completions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(completions)))
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        if !control::is_control_file(&params.text_document.uri) {
            return Ok(None);
        }

        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let file = match files.get(&params.text_document.uri) {
            Some(f) => *f,
            None => return Ok(None),
        };

        let parsed = workspace.get_parsed_control(file);
        let source_text = workspace.source_text(file);

        let mut actions = Vec::new();

        // Check for field casing issues
        if let Ok(control) = parsed.to_result() {
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    if let Some(field_name) = entry.key() {
                        if let Some(standard_name) = control::get_standard_field_name(&field_name) {
                            if field_name != standard_name {
                                // Get the field's position from the entry
                                let entry_range = entry.text_range();
                                let field_range = TextRange::new(
                                    entry_range.start(),
                                    entry_range.start() + TextSize::of(field_name.as_str()),
                                );
                                let lsp_range = text_range_to_lsp_range(&source_text, field_range);

                                // Create a code action to fix the casing
                                let edit = TextEdit {
                                    range: lsp_range,
                                    new_text: standard_name.to_string(),
                                };

                                let workspace_edit = WorkspaceEdit {
                                    changes: Some(
                                        vec![(params.text_document.uri.clone(), vec![edit])]
                                            .into_iter()
                                            .collect(),
                                    ),
                                    ..Default::default()
                                };

                                let action = CodeAction {
                                    title: format!(
                                        "Fix field casing: {} -> {}",
                                        field_name, standard_name
                                    ),
                                    kind: Some(CodeActionKind::QUICKFIX),
                                    edit: Some(workspace_edit),
                                    ..Default::default()
                                };

                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    }
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        workspace: Arc::new(Mutex::new(Workspace::new())),
        files: Arc::new(Mutex::new(HashMap::new())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_completion_returns_control_completions() {
        // Test that the completion method properly uses the control module
        let uri = Url::parse("file:///path/to/debian/control").unwrap();
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        // We can't easily test the actual completion without a full LSP setup,
        // but we can verify the control module works
        let completions = control::get_completions(
            &params.text_document_position.text_document.uri,
            params.text_document_position.position,
        );
        assert!(!completions.is_empty());
    }

    #[tokio::test]
    async fn test_completion_returns_none_for_non_control_files() {
        let uri = Url::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = control::get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_control_module_integration() {
        // Test that the control module is properly integrated
        let control_uri = Url::parse("file:///path/to/debian/control").unwrap();
        let non_control_uri = Url::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        // Control file should return completions
        let completions = control::get_completions(&control_uri, position);
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.label == "Source"));
        assert!(completions.iter().any(|c| c.label == "debhelper-compat"));

        // Non-control file should return no completions
        let completions = control::get_completions(&non_control_uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_workspace_integration() {
        // Test that the workspace can parse control files
        let mut workspace = workspace::Workspace::new();
        let url = Url::parse("file:///debian/control").unwrap();
        let content = "source: test-package\nMaintainer: Test <test@example.com>\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        // Should parse correctly
        assert!(parsed.errors().is_empty());

        if let Ok(control) = parsed.to_result() {
            let mut field_names = Vec::new();
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    if let Some(name) = entry.key() {
                        field_names.push(name);
                    }
                }
            }
            assert!(field_names.contains(&"source".to_string()));
            assert!(field_names.contains(&"Maintainer".to_string()));
        }
    }

    #[test]
    fn test_field_casing_detection() {
        // Test that we can detect incorrect field casing
        use control::get_standard_field_name;

        // Test correct casing - should return the same
        assert_eq!(get_standard_field_name("Source"), Some("Source"));
        assert_eq!(get_standard_field_name("Package"), Some("Package"));
        assert_eq!(get_standard_field_name("Maintainer"), Some("Maintainer"));

        // Test incorrect casing - should return the standard form
        assert_eq!(get_standard_field_name("source"), Some("Source"));
        assert_eq!(get_standard_field_name("package"), Some("Package"));
        assert_eq!(get_standard_field_name("maintainer"), Some("Maintainer"));
        assert_eq!(get_standard_field_name("MAINTAINER"), Some("Maintainer"));

        // Test unknown fields - should return None
        assert_eq!(get_standard_field_name("UnknownField"), None);
        assert_eq!(get_standard_field_name("random"), None);
    }
}
