//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::NumberOrString;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

mod control;
mod position;
mod workspace;

use position::{lsp_range_to_text_range, text_range_to_lsp_range};
use std::collections::HashMap;
// Removed unused imports - TextRange and TextSize are no longer used in main.rs
use workspace::Workspace;

/// Check if two LSP ranges overlap
fn range_overlaps(a: &Range, b: &Range) -> bool {
    // Check if range a starts before b ends and b starts before a ends
    (a.start.line < b.end.line
        || (a.start.line == b.end.line && a.start.character <= b.end.character))
        && (b.start.line < a.end.line
            || (b.start.line == a.end.line && b.start.character <= a.end.character))
}

struct Backend {
    client: Client,
    workspace: Arc<Mutex<Workspace>>,
    files: Arc<Mutex<HashMap<Uri, workspace::SourceFile>>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct InlayHintParams {
    path: String,
}

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
                format!("file opened: {:?}", params.text_document.uri),
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

            // Publish diagnostics
            let diagnostics = workspace.get_diagnostics(file);
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                .await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file changed: {:?}", params.text_document.uri),
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

                // Publish diagnostics
                let diagnostics = workspace.get_diagnostics(file);
                self.client
                    .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                    .await;
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

        let source_text = workspace.source_text(file);

        let mut actions = Vec::new();

        // Check for field casing issues - only process fields in the requested range
        let text_range = lsp_range_to_text_range(&source_text, &params.range);

        for issue in workspace.find_field_casing_issues(file, Some(text_range)) {
            let lsp_range = text_range_to_lsp_range(&source_text, issue.field_range);

            // Double-check it's within the requested range (should always be true)
            if range_overlaps(&lsp_range, &params.range) {
                // Check if there's a matching diagnostic in the context
                let matching_diagnostics = params
                    .context
                    .diagnostics
                    .iter()
                    .filter(|d| {
                        d.range == lsp_range
                            && d.code == Some(NumberOrString::String("field-casing".to_string()))
                    })
                    .cloned()
                    .collect::<Vec<_>>();

                // Create a code action to fix the casing
                let edit = TextEdit {
                    range: lsp_range,
                    new_text: issue.standard_name.clone(),
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
                        issue.field_name, issue.standard_name
                    ),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(workspace_edit),
                    diagnostics: if !matching_diagnostics.is_empty() {
                        Some(matching_diagnostics)
                    } else {
                        None
                    },
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
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
        let uri = str::parse("file:///path/to/debian/control").unwrap();
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
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = control::get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_control_module_integration() {
        // Test that the control module is properly integrated
        let control_uri = str::parse("file:///path/to/debian/control").unwrap();
        let non_control_uri = str::parse("file:///path/to/other.txt").unwrap();
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
        let url = str::parse("file:///debian/control").unwrap();
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
