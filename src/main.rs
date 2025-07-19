//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod control;

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

        let completions = control::get_completions(&uri, position);

        if completions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(completions)))
        }
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
}
