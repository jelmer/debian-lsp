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

        if !uri.path().ends_with("/control") && !uri.path().contains("/debian/control") {
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
